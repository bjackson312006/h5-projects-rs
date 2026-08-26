//! CAN stuff.

use can_handler::NerCan;
use embassy_stm32::can::Frame;

embassy_stm32::bind_interrupts!(struct Irqs {
    FDCAN2_IT0 => embassy_stm32::can::IT0InterruptHandler<embassy_stm32::peripherals::FDCAN2>;
    FDCAN2_IT1 => embassy_stm32::can::IT1InterruptHandler<embassy_stm32::peripherals::FDCAN2>;
});

mod channels {
    use super::Frame;
    use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
    use embassy_sync::channel::Channel;

    /// Channel for frames that we recieve.
    pub(super) static INCOMING: Channel<ThreadModeRawMutex, Frame, 16> = Channel::new();
    /// Channel for frames we queue to send.
    pub(super) static OUTGOING: Channel<ThreadModeRawMutex, Frame, 16> = Channel::new();
}
/// Add a frame to the outgoing CAN channel.
pub async fn send(frame: Frame) { 
    match channels::OUTGOING.try_send(frame) {
        Ok(_) => { return; },
        Err(_) => {
            defmt::warn!("Tried to add a frame to the OUTGOING Channel, but the Channel was full. This is not a failure, because we will .await until the Channel is able to accept the frame. However, consider increasing the capacity of the Channel if this is occurring often.");
            channels::OUTGOING.send(frame).await
        }
    }
}
/// Get a frame from the incoming CAN channel.
/// (this doesn't need to check for an error because of `receive()` is empty there is no problem, it just means there is no pending messages)
pub async fn recieve() -> Frame { channels::INCOMING.receive().await }

/// Initializes CAN and starts up the CAN handler.
#[embassy_executor::task]
pub async fn can_task(spawner: embassy_executor::Spawner, r: crate::CanResources) {
    use core::num::{NonZeroU16, NonZeroU8};
    use embassy_stm32::can::CanConfigurator;
    use embassy_stm32::can::config::TxBufferMode;
    use embassy_stm32::can::config::NominalBitTiming;

    let configurator = CanConfigurator::new(r.can, r.can_rx, r.can_tx, Irqs);
    let mut ner_can = NerCan::init(configurator);

    // get the config from ner_can and update it (there are some things that need to be overwritten to be the same as `MX_FDCAN2_Init()` from TSECU-Shepherd)
    let config = ner_can.can_configurator.config();
        // ner_can uses `set_bitrate(500_000)` but we need to override that because it picks a different sample point
        // this should match the 75% sample point from TSECU-Shepherd but should still also be 500 kbit/s?
        config.set_nominal_bit_timing(NominalBitTiming {
            prescaler: NonZeroU16::new(8).unwrap(),
            seg1: NonZeroU8::new(11).unwrap(),
            seg2: NonZeroU8::new(4).unwrap(),
            sync_jump_width: NonZeroU8::new(1).unwrap(),
        })
        // TransmitPause = DISABLE in TSECU-Shepherd
        .set_transmit_pause(false)
        // ProtocolException = DISABLE in TSECU-Shepherd
        .set_protocol_exception_handling(false)
        // TxFifoQueueMode = FDCAN_TX_FIFO_OPERATION in TSECU-Shepherd
        .set_tx_buffer_mode(TxBufferMode::Fifo);
    ner_can.can_configurator.set_config(config);

    // u_TODO probably should add can fitlers and such here

    spawner.spawn(can_handler::can_handler(ner_can.can_configurator, channels::INCOMING.sender(), channels::OUTGOING.receiver()).expect("Failed to spawn can_handler::can_handler()."),);
}
