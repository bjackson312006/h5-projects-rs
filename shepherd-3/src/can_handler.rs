//! Generic CAN handler for NER STM32H5 firmware projects.
//!
//! u_TODO - this is a local copy of `platform/can-handler/src/lib.rs` from `firmware-rs`,
//! vendored in so the CAN bring-up problems can be debugged without pushing to that repo.
//! Once things work, the changes here should go back upstream and this module should be
//! deleted in favor of the `can-handler` crate again.
//!
//! This crate wraps Embassy's `embassy-stm32` FDCAN peripheral to provide a
//! ready-to-use Classical CAN configuration and an [`embassy_executor`] task
//! ([`can_handler`]) that bridges the CAN bus with the rest of a user program
//! over [`embassy_sync`] channels.
//!
//! The bus is configured for Classical CAN at 500 kbit/s.  
use defmt::{warn, info};
use embassy_stm32::can::filter::FilterType::{DedicatedDual, DedicatedSingle};
use embassy_stm32::can::filter::{
    Action, ExtendedFilter, ExtendedFilterSlot, StandardFilter, StandardFilterSlot,
};
use embassy_stm32::can::{CanConfigurator, CanRx, CanTx, Frame, Properties};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::channel::{Receiver, Sender};
use embassy_time::Timer;
use embedded_can::{ExtendedId, StandardId};

use heapless::Vec;

pub struct NerCan {
    pub can_configurator: CanConfigurator<'static>,
    used_std_slots: Vec<StandardFilterSlot, 28>,
    used_ext_slots: Vec<ExtendedFilterSlot, 28>,
}

impl NerCan {
    /// This is the CAN configuration to be used by most NER Projects.
    /// This is for optional use to pass into the can_handler task to facilitate initialization
    ///
    /// The configuration sets:
    /// - Automatic bus-off recovery enabled.
    /// - Automatic retransmission disabled.
    /// - Classical CAN framing only (no CAN FD).
    /// - A clock divider of 1 and the data bit timing required for 500 kbit/s.
    /// - Transmit pause enabled.
    /// - A global filter that rejects all frames by default.
    ///
    /// ** It is expected that the user manually configures the CAn Std and Extended Filters before running the can_handler task
    /// ** Hardcodes bitrate to 500 kbit/s, if CAN sampling causes issues, this must be adjusted in this lib
    pub fn init(mut can_configurator: CanConfigurator<'static>) -> Self {
        use embassy_stm32::can::config::*;

        let can_config = FdCanConfig::default()
            .set_automatic_bus_off_recovery(true)
            .set_automatic_retransmit(true)
            .set_frame_transmit(FrameTransmissionConfig::ClassicCanOnly)
            .set_transmit_pause(true)
            .set_global_filter(GlobalFilter::reject_all());
        can_configurator.set_config(can_config);
        can_configurator.set_bitrate(500_000);

        Self {
            can_configurator,
            used_std_slots: Vec::new(),
            used_ext_slots: Vec::new(),
        }
    }

    /// Sets adds a new CAN Standard Filter at the given slot
    /// NOTE: will panic if the given slot is already in use
    pub fn add_standard_filter(
        mut self,
        std_filter_slot: StandardFilterSlot,
        std_id1: u16,
        std_id2: Option<u16>,
    ) -> Self {
        if self.used_std_slots.contains(&std_filter_slot) {
            panic!("The selected CAN Standard Filter Slot is already in use.");
        }

        let mut std = StandardFilter::default();
        match std_id2 {
            Some(id2) => {
                std.filter = DedicatedDual(
                    StandardId::new(std_id1).unwrap(),
                    StandardId::new(id2).unwrap(),
                );
            }
            None => {
                std.filter = DedicatedSingle(StandardId::new(std_id1).unwrap());
            }
        }
        std.action = Action::StoreInFifo0;
        self.can_configurator
            .properties()
            .set_standard_filter(std_filter_slot, std);
        let _ = self.used_std_slots.push(std_filter_slot);

        self
    }

    /// Sets adds a new CAN Extended Filter at the given slot
    /// NOTE: will panic if the given slot is already in use
    pub fn add_extended_filter(
        mut self,
        ext_filter_slot: ExtendedFilterSlot,
        ext_id1: u32,
        ext_id2: Option<u32>,
    ) -> Self {
        if self.used_ext_slots.contains(&ext_filter_slot) {
            panic!("The selected CAN Extended Filter Slot is already in use.");
        }

        let mut ext = ExtendedFilter::default();
        match ext_id2 {
            Some(id2) => {
                ext.filter = DedicatedDual(
                    ExtendedId::new(ext_id1).unwrap(),
                    ExtendedId::new(id2).unwrap(),
                );
            }
            None => {
                ext.filter = DedicatedSingle(ExtendedId::new(ext_id1).unwrap());
            }
        }
        ext.action = Action::StoreInFifo0;
        self.can_configurator
            .properties()
            .set_extended_filter(ext_filter_slot, ext);
        let _ = self.used_ext_slots.push(ext_filter_slot);

        self
    }
}

/// How often [`can_props`] publishes a CAN health sample.
const PROPS_SAMPLE_PERIOD_MS: u64 = 500;

/// Number of hardware TX mailboxes (embassy's `TX_FIFO_MAX` for this message RAM layout).
const TX_MAILBOX_COUNT: usize = 3;

/// CAN handler Embassy task for generic use in STM32H5 projects.
///
/// Puts the configurator into normal mode, splits the peripheral into its TX, RX and
/// properties halves, and hands each one to its own task. Splitting means a TX that
/// blocks (mailboxes full and not draining) can no longer starve RX, and the health
/// sampler in [`can_props`] keeps publishing even when both directions are wedged.
///
/// **The `sender` and `receiver` are not intended to derive from the same channel
///
/// - `sender` passes on CAN frames received from the bus so they can be parsed
///   by the user program.
/// - `receiver` dispatches CAN frames queued by other threads in the user
///   program for transmission onto the bus.
///
#[embassy_executor::task]
pub async fn can_handler(
    spawner: embassy_executor::Spawner,
    can_configurator: CanConfigurator<'static>,
    sender: Sender<'static, ThreadModeRawMutex, Frame, 256>,
    receiver: Receiver<'static, ThreadModeRawMutex, Frame, 256>,
) {
    // Starts Classical CAN transmission and receival
    let can = can_configurator.into_normal_mode();

    let (tx, rx, props) = can.split();

    spawner.spawn(can_tx(tx, receiver).expect("Failed to spawn can_handler::can_tx()."));
    spawner.spawn(can_rx(rx, sender).expect("Failed to spawn can_handler::can_rx()."));
    spawner.spawn(can_props(props).expect("Failed to spawn can_handler::can_props()."));

    info!("CAN split into tx/rx/props tasks. (can_handler)");
}

/// Drains the outgoing channel onto the bus.
///
/// NOTE: this must never return. `CanTx` and `CanRx` are what hold the driver's internal
/// reference count above zero; once the last of them is dropped, embassy disconnects the
/// CAN pins and disables the peripheral. `Properties` carries no such reference, so
/// [`can_props`] would happily go on sampling a dead peripheral.
#[embassy_executor::task]
pub async fn can_tx(
    mut tx: CanTx<'static>,
    receiver: Receiver<'static, ThreadModeRawMutex, Frame, 256>,
) -> ! {

    let mut send_count: u32 = 0;

    loop {
        let frame = receiver.receive().await;

        match tx.write(&frame).await {
            Some(frame) => { 
                crate::can::send(frame).await;
            },
            None => send_count += 1,
        }

        defmt_monitor::monitor!("CanDebug/send_count", desc = "Send count", "{}", send_count);
    }
}

/// Passes frames received off the bus to the incoming channel.
///
/// NOTE: must never return, for the reason described on [`can_tx`].
#[embassy_executor::task]
pub async fn can_rx(
    mut rx: CanRx<'static>,
    sender: Sender<'static, ThreadModeRawMutex, Frame, 256>,
) -> ! {

    let mut rx_count: u32 = 0;
    let mut rx_err_count: u32 = 0;

    loop {
        match rx.read().await {
            Ok(can_recv) => { 
                sender.send(can_recv.frame).await;
                rx_count += 1;
            },
            Err(err) => { 
                warn!("Bus error! {}", err);
                rx_err_count += 1;
            },
        }

        defmt_monitor::monitor!("CanDebug/rx_count", desc = "RX count", "{}", rx_count);
        defmt_monitor::monitor!("CanDebug/rx_err_count", desc = "RX err count", "{}", rx_err_count);
    }
}

/// Publishes peripheral-level CAN health on a fixed period.
///
/// This owns no part of the datapath, so it keeps sampling regardless of what TX and RX
/// are doing. That is the point: if the bus wedges, this is what shows whether the
/// peripheral is erroring, bus-off, or not transmitting at all.
///
/// The `Properties` handle is taken but deliberately unused. Several of the fields
/// sampled here are read-to-clear (`PSR.LEC` resets to `NO_CHANGE` on read, `ECR.CEL`
/// resets to 0), and `Properties` reads PSR again inside `bus_error_mode()` and ECR again
/// inside each of `tx_error_count()`/`rx_error_count()`. Mixing the two APIs would throw
/// away the very fields we are trying to sample, so everything below comes from one
/// snapshot taken through the PAC instead.
///
/// NOTE: reading PSR here consumes `LEC` for everyone, including embassy's own
/// `curr_error()` path behind [`can_rx`]. That is harmless while the global filter is
/// `reject_all()` and no filters are configured (so RX never runs), but it will make
/// `Bus error!` reports lossy once RX is live.
#[embassy_executor::task]
pub async fn can_props(_props: Properties) -> ! {
    use core::sync::atomic::Ordering;
    use embassy_stm32::can::enums::BusErrorMode;
    use embassy_stm32::pac;

    let regs = pac::FDCAN2;

    loop {
        // Whole snapshot up front, exactly one read per register.
        let psr = regs.psr().read();
        let ecr = regs.ecr().read();
        let cccr = regs.cccr().read();
        let ir = regs.ir().read();
        let ie = regs.ie().read();
        let ils = regs.ils().read();
        let ile = regs.ile().read();
        let txfqs = regs.txfqs().read();
        let txbrp = regs.txbrp().read();
        let txbto = regs.txbto().read();
        let txbcf = regs.txbcf().read();

        // Same derivation embassy's `Properties::bus_error_mode()` uses, off our own read.
        let bus_error_mode = match (psr.bo(), psr.ep()) {
            (false, false) => BusErrorMode::ErrorActive,
            (false, true) => BusErrorMode::ErrorPassive,
            (true, _) => BusErrorMode::BusOff,
        };

        // One bit per hardware TX mailbox.
        let mut pending_mask = 0_u8;
        let mut occurred_mask = 0_u8;
        let mut cancelled_mask = 0_u8;
        let mut pending_count = 0_u8;
        for i in 0..TX_MAILBOX_COUNT {
            if txbrp.trp(i) {
                pending_mask |= 1_u8 << i;
                pending_count += 1_u8;
            }
            if txbto.to(i) {
                occurred_mask |= 1_u8 << i;
            }
            if txbcf.cf(i) {
                cancelled_mask |= 1_u8 << i;
            }
        }

        // Error counters and protocol status.
        defmt_monitor::monitor!("CanDebug/tx_error_count", desc = "FDCAN TEC (ECR.TEC). Climbs by 8 per failed transmission; >255 means bus-off.", "{=u8}", ecr.tec());
        defmt_monitor::monitor!("CanDebug/rx_error_count", desc = "FDCAN REC (ECR.REC).", "{=u8}", ecr.rec());
        defmt_monitor::monitor!("CanDebug/bus_error_mode", desc = "FDCAN bus error state, from PSR.BO/PSR.EP.", "{}", bus_error_mode);
        defmt_monitor::monitor!("CanDebug/error_warning", desc = "PSR.EW. An error counter has passed 96.", "{=bool}", psr.ew());
        defmt_monitor::monitor!("CanDebug/error_logging_count", desc = "ECR.CEL, cleared on read, so this is the number of protocol errors since the previous sample.", "{=u8}", ecr.cel());
        defmt_monitor::monitor!("CanDebug/last_error_code", desc = "PSR.LEC since the previous sample. ACK means nobody on the bus acknowledged our frame; NO_CHANGE means no bus event at all.", "{}", psr.lec());
        defmt_monitor::monitor!("CanDebug/node_activity", desc = "PSR.ACT. SYNC=still synchronizing to the bus, IDLE=neither sending nor receiving, RX/TX=actively on the bus.", "{}", psr.act());

        // TX mailbox occupancy. `tx_pending_*` staying high while `tx_occurred_mask` stays
        // 0 means frames are queued but never make it onto the wire.
        defmt_monitor::monitor!("CanDebug/tx_pending_mask", desc = "TXBRP, one bit per mailbox: a transmission is requested and not yet finished.", "{=u8}", pending_mask);
        defmt_monitor::monitor!("CanDebug/tx_pending_count", desc = "Number of TX mailboxes with a pending request, 0 to 3.", "{=u8}", pending_count);
        defmt_monitor::monitor!("CanDebug/tx_occurred_mask", desc = "TXBTO, one bit per mailbox: a frame was successfully transmitted. Stays 0 if nothing has ever reached the bus.", "{=u8}", occurred_mask);
        defmt_monitor::monitor!("CanDebug/tx_cancelled_mask", desc = "TXBCF, one bit per mailbox: a transmission was cancelled, which is what DAR=1 does to a frame that fails its single attempt.", "{=u8}", cancelled_mask);
        defmt_monitor::monitor!("CanDebug/tx_fifo_full", desc = "TXFQS.TFQF. No free mailbox.", "{=bool}", txfqs.tfqf());
        defmt_monitor::monitor!("CanDebug/tx_fifo_free_level", desc = "TXFQS.TFFL, free mailboxes. Reads 0 in queue mode (TXBC.TFQM=1), so 0 here alongside tx_fifo_full=false confirms queue rather than FIFO mode.", "{=u8}", txfqs.tffl());
        defmt_monitor::monitor!("CanDebug/tx_put_index", desc = "TXFQS.TFQPI, mailbox the next write lands in.", "{=u8}", txfqs.tfqpi());

        // Interrupt path. A rising it0_irq_count is the only proof the ISR runs at all;
        // ir_tc_latched stuck true is proof it does not.
        defmt_monitor::monitor!("CanDebug/it0_irq_count", desc = "Times the FDCAN2 IT0 ISR has fired. Flat at 0 while frames are queued means TX completions never wake the writer.", "{=u32}", crate::can::IT0_IRQ_COUNT.load(Ordering::Relaxed));
        defmt_monitor::monitor!("CanDebug/it1_irq_count", desc = "Times the FDCAN2 IT1 ISR has fired. Should stay 0: ILS routes everything to line 0.", "{=u32}", crate::can::IT1_IRQ_COUNT.load(Ordering::Relaxed));
        defmt_monitor::monitor!("CanDebug/ir_tc_latched", desc = "IR.TC still set at sample time. Embassy's ISR clears this on entry, so persistently true means the ISR is not running.", "{=bool}", ir.tc());
        defmt_monitor::monitor!("CanDebug/ir", desc = "Raw FDCAN IR, all latched interrupt flags.", "{=u32}", ir.0);
        defmt_monitor::monitor!("CanDebug/ie", desc = "Raw FDCAN IE, enabled interrupt sources. Expect TCE, RFNE0/1 and BOE set.", "{=u32}", ie.0);
        defmt_monitor::monitor!("CanDebug/ils", desc = "Raw FDCAN ILS, interrupt line select. Expect 0: embassy never writes this and services line 0.", "{=u32}", ils.0);
        defmt_monitor::monitor!("CanDebug/ile", desc = "Raw FDCAN ILE, interrupt line enable. Expect both EINT0 and EINT1 set.", "{=u32}", ile.0);

        // Operating mode, to confirm the peripheral is actually on the bus.
        defmt_monitor::monitor!("CanDebug/cccr_init", desc = "CCCR.INIT. True means the peripheral is held out of bus traffic, which hardware does on bus-off.", "{=bool}", cccr.init());
        defmt_monitor::monitor!("CanDebug/cccr_dar", desc = "CCCR.DAR. True means automatic retransmission is disabled, so a failed frame is discarded after one attempt.", "{=bool}", cccr.dar());
        defmt_monitor::monitor!("CanDebug/cccr_mon", desc = "CCCR.MON. Bus monitoring mode; true means we never drive the bus dominant.", "{=bool}", cccr.mon());
        defmt_monitor::monitor!("CanDebug/cccr_test", desc = "CCCR.TEST. True in either loopback mode.", "{=bool}", cccr.test());

        Timer::after_millis(PROPS_SAMPLE_PERIOD_MS).await;
    }
}
