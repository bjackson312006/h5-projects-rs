//! Module for controlling the Segments and their ADBMS6830B chips.
//! 
//! For context, there are 5 segments, each with two ADBMS6830B chips. So, there are 10 ADBMS6830B chips total.

use embassy_time::Timer;

mod voltages;
mod cache;
mod chips;

embassy_stm32::bind_interrupts!(struct Irqs {
    GPDMA1_CHANNEL0 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH0>;
    GPDMA1_CHANNEL1 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH1>;
    GPDMA1_CHANNEL2 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH2>;
    GPDMA1_CHANNEL3 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH3>;
});

mod alias {
    use embedded_hal_bus::spi::ExclusiveDevice;
    use embassy_time::Delay;
    use embassy_stm32::{
        mode::Async,
        gpio::Output,
        spi::{ Spi, mode::Master },
    };
    use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;

    /// Number of ADBMS6830B chips we have.
    pub const ADBMS6830B_NUM_CHIPS: usize = const { super::chips::ChipId::VARIANT_COUNT };

    /// Type alias representing a SPI controller that implements `SpiDevice` from `embedded_hal_async`.
    /// This is just a single SPI controller with a CS pin.
    pub type SpiDevice = ExclusiveDevice<Spi<'static, Async, Master>, Output<'static>, Delay>;

    /// The error type our `SpiDevice` produces.
    #[allow(unused)]
    pub type SpiError = <SpiDevice as embedded_hal_async::spi::ErrorType>::Error;

    /// Type alias representing an IsoSPI Line.
    /// 
    /// Each line can go up to `ADBMS6830B_NUM_CHIPS` chips, but the actual number of chips they have is dynamic at runtime and is managed by the `Service`.
    pub type Line = adbms6830b::line::Line<SpiDevice, ADBMS6830B_NUM_CHIPS>;

    /// Type alias representing our adbms6830b Service configuration.
    pub type Service = adbms6830b::turnkey::service::Service<ThreadModeRawMutex, SpiDevice, ADBMS6830B_NUM_CHIPS>;
}

/// Errors that may occur when configuring Segments.
#[derive(Copy, Clone, Debug)]
#[derive(defmt::Format)]
pub enum ConfigureError {
    /// We exceeded the number of attempts for waking up Segments and verifying our configs got written.
    UnverifiedWakeup,
    /// Failed to send command to start conversions.
    ConversionStartFailed,
}

/// Guy in charge of the segments.
pub struct Segments {
    service: alias::Service,

    cache: cache::CacheData<{ alias::ADBMS6830B_NUM_CHIPS }>,
}
impl Segments {
    /// Initializes our `Segments`. AKA inits the two isoSPI lines. Doesn't start up the task or set any runtime config registers though.
    /// 
    /// ### Parameters
    /// - `r_linea`: pins and other hardware resources for Line A
    /// - `r_lineb`: pins and other hardware resources for Line B
    pub fn new(r_linea: crate::SegmentIsoSpiLineAResources, r_lineb: crate::SegmentIsoSpiLineBResources) -> Self {
        use embedded_hal_bus::spi::ExclusiveDevice;
        use embassy_time::{Delay};
        use adbms6830b::turnkey::service::service_config::{
            ServiceConfig,
            SEGMENT_ISOSPI_EVAL_PERIOD_MS,
            SEGMENT_ISOSPI_MAX_FAILED_VERIFICATION_ATTEMPTS,
            SEGMENT_ISOSPI_MAX_SPLIT_ATTEMPTS,
            SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL,
            SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW,
            SEGMENT_ISOSPI_PEC_FAILURE_RATIO_PCT,
            SEGMENT_ISOSPI_RECOVERY_STARTUP_TIME_MS,
            SERVICE_FREQUENCY_MS,
        };

        let mut spi_config = embassy_stm32::spi::Config::default();
        spi_config.frequency = embassy_stm32::time::mhz(1);

        let linea_spi = embassy_stm32::spi::Spi::new(
            r_linea.linea_spi,
            r_linea.linea_sck,
            r_linea.linea_mosi,
            r_linea.linea_miso,
            r_linea.linea_tx_dma,
            r_linea.linea_rx_dma,
            Irqs,
            spi_config,
        );
        let linea_cs = embassy_stm32::gpio::Output::new(r_linea.linea_cs, embassy_stm32::gpio::Level::High, embassy_stm32::gpio::Speed::High);
        let linea_spi = ExclusiveDevice::new(linea_spi, linea_cs, Delay).unwrap();
        let line_a: alias::Line = adbms6830b::line::Line::new(linea_spi);

        let lineb_spi = embassy_stm32::spi::Spi::new(
            r_lineb.lineb_spi,
            r_lineb.lineb_sck,
            r_lineb.lineb_mosi,
            r_lineb.lineb_miso,
            r_lineb.lineb_tx_dma,
            r_lineb.lineb_rx_dma,
            Irqs,
            spi_config,
        );
        let lineb_cs = embassy_stm32::gpio::Output::new(r_lineb.lineb_cs, embassy_stm32::gpio::Level::High, embassy_stm32::gpio::Speed::High);
        let lineb_spi: alias::SpiDevice = ExclusiveDevice::new(lineb_spi, lineb_cs, Delay).unwrap();
        let line_b: alias::Line = adbms6830b::line::Line::new(lineb_spi);

        let service: alias::Service = alias::Service::new(line_a, line_b, ServiceConfig {
                // ik could just use `..Default::default()` but this is easier if we wanna change config in the future
                segment_isospi_eval_period_ms: SEGMENT_ISOSPI_EVAL_PERIOD_MS,
                service_frequency_ms: SERVICE_FREQUENCY_MS,
                segment_isospi_min_attempts_for_fail: SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL,
                segment_isospi_pec_failure_ratio_pct: SEGMENT_ISOSPI_PEC_FAILURE_RATIO_PCT,
                segment_isospi_min_attempts_to_open_window: SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW,
                segment_isospi_max_split_attempts: SEGMENT_ISOSPI_MAX_SPLIT_ATTEMPTS,
                segment_isospi_max_failed_verification_attempts: SEGMENT_ISOSPI_MAX_FAILED_VERIFICATION_ATTEMPTS,
                segment_isospi_recovery_startup_time_ms: SEGMENT_ISOSPI_RECOVERY_STARTUP_TIME_MS,
        });

        Self { 
            service,
            cache: cache::CacheData::new(),
        }
    }

    // pub fn test(&self) {
    //     self.cache.rdraxa.update(&self.service).await
    // }
}

#[embassy_executor::task]
pub async fn segments_task(r_linea: crate::SegmentIsoSpiLineAResources, r_lineb: crate::SegmentIsoSpiLineBResources) {
    let segments = Segments::new(r_linea, r_lineb);

    // this runs the service. it never returns after you call it. the service will run at the configured `service_frequency_ms`. The closure allows you to
    // execute code every time the service runs (intended to be used for diagnostics, but other stuff can go in there too)
    segments.service.run(
        // SERVICE DIAGNOSTICS! this runs every service cycle with a new `diagnostics` provided from that cycle
        async |diagnostics| {
        // timestamp for measuring how long these diagnostic logs themselves take. see the `Timing/diagnostics_overhead` topic near the bottom of closure
        let diagnostics_started = embassy_time::Instant::now();

        // accumulator diagnostics (not per-chip ones)
        let accumulator = diagnostics.accumulator();
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PreviousState", desc = "State of the accumulator accumulator prior to this Service cycle. This is the state failed, attempts, and failure_pct were gathered under.", "{}", accumulator.previous_state());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/State", desc = "Current state of the accumulator window. This is the state resulting from the failed, attempts, and failure_pct values.", "{}", accumulator.state());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/FailurePctThreshold", desc = "Percentage of reads that must fail their PEC for a chip to be considered as \"failing\". Note: This is a constant value!", "{=u8}", accumulator.failure_pct_threshold());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/AccumulatorWindowPeriod", desc = "How long the accumulator evaluation window lasts, in ms. Basically, after a window opens, this is how long the window stays open to gather PEC data. Note: This is a constant value!", "{=u64}", accumulator.accumulator_window_period());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/MinAttemptsForFail", desc = "Fewest reads a chip must have taken part in before its failure rate is actually considered as meaning anything. Note: This is a constant value!", "{=usize}", accumulator.min_attempts_for_fail());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/BelowMinAttemptsToOpenWindowCount", desc = "This counts the number of times a chip has passed over opening a window because it hadn't taken part in SEGMENT_ISOSPI_MIN_ATTEMPTS_TO_OPEN_WINDOW reads since the last time the Service has run.", "{=usize}", accumulator.below_min_attempts_to_open_window_count());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/BelowMinAttemptsForFailCount", desc = "This counts the number of times a chip could not be judged failed or not because it hadn't taken part in SEGMENT_ISOSPI_MIN_ATTEMPTS_FOR_FAIL reads over the whole window.", "{=usize}", accumulator.below_min_attempts_for_fail_count());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/MinAttemptsToOpenWindow", desc = "Fewest reads in a single update before that update's failure rate can open a window. Note: This is a constant value!", "{=usize}", accumulator.min_attempts_to_open_window());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PecMask", desc = "Current PEC mask state.", "{}", accumulator.pec_mask());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/RecoveryStartupTime", desc = "Length of grace period that occurs after startup or after a sleep, where the accumulator doesn't accumulate PEC errors. Note: This is a constant value!", "{=u64}", accumulator.recovery_startup_time());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/UpdatesWhileMaskedCount", desc = "This counts the number of times update_chips() (and therefore update() itself) has run while a PEC mask is active.", "{=usize}", accumulator.updates_while_masked_count());

        // accumulator diagnostics (.failed() ones).
        let failed = accumulator.failed();
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip0/failed", desc = "Total PEC failures counted for this chip during this window.", "{=usize}", failed[0]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip1/failed", desc = "Total PEC failures counted for this chip during this window.", "{=usize}", failed[1]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip2/failed", desc = "Total PEC failures counted for this chip during this window.", "{=usize}", failed[2]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip3/failed", desc = "Total PEC failures counted for this chip during this window.", "{=usize}", failed[3]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip4/failed", desc = "Total PEC failures counted for this chip during this window.", "{=usize}", failed[4]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip5/failed", desc = "Total PEC failures counted for this chip during this window.", "{=usize}", failed[5]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip6/failed", desc = "Total PEC failures counted for this chip during this window.", "{=usize}", failed[6]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip7/failed", desc = "Total PEC failures counted for this chip during this window.", "{=usize}", failed[7]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip8/failed", desc = "Total PEC failures counted for this chip during this window.", "{=usize}", failed[8]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip9/failed", desc = "Total PEC failures counted for this chip during this window.", "{=usize}", failed[9]);

        // accumulator diagnostics (.attempts() ones).
        let attempts = accumulator.attempts();
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip0/attempts", desc = "Total read attempts counted for this chip during this window.", "{=usize}", attempts[0]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip1/attempts", desc = "Total read attempts counted for this chip during this window.", "{=usize}", attempts[1]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip2/attempts", desc = "Total read attempts counted for this chip during this window.", "{=usize}", attempts[2]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip3/attempts", desc = "Total read attempts counted for this chip during this window.", "{=usize}", attempts[3]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip4/attempts", desc = "Total read attempts counted for this chip during this window.", "{=usize}", attempts[4]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip5/attempts", desc = "Total read attempts counted for this chip during this window.", "{=usize}", attempts[5]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip6/attempts", desc = "Total read attempts counted for this chip during this window.", "{=usize}", attempts[6]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip7/attempts", desc = "Total read attempts counted for this chip during this window.", "{=usize}", attempts[7]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip8/attempts", desc = "Total read attempts counted for this chip during this window.", "{=usize}", attempts[8]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip9/attempts", desc = "Total read attempts counted for this chip during this window.", "{=usize}", attempts[9]);

        // accumulator diagnostics (.failure_pct() ones).
        let failure_pct = accumulator.failure_pct();
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip0/failure_pct", desc = "This chip's PEC failure rate over the current window as a percentage (0 - 100).", "{=u8}", failure_pct[0]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip1/failure_pct", desc = "This chip's PEC failure rate over the current window as a percentage (0 - 100).", "{=u8}", failure_pct[1]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip2/failure_pct", desc = "This chip's PEC failure rate over the current window as a percentage (0 - 100).", "{=u8}", failure_pct[2]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip3/failure_pct", desc = "This chip's PEC failure rate over the current window as a percentage (0 - 100).", "{=u8}", failure_pct[3]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip4/failure_pct", desc = "This chip's PEC failure rate over the current window as a percentage (0 - 100).", "{=u8}", failure_pct[4]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip5/failure_pct", desc = "This chip's PEC failure rate over the current window as a percentage (0 - 100).", "{=u8}", failure_pct[5]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip6/failure_pct", desc = "This chip's PEC failure rate over the current window as a percentage (0 - 100).", "{=u8}", failure_pct[6]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip7/failure_pct", desc = "This chip's PEC failure rate over the current window as a percentage (0 - 100).", "{=u8}", failure_pct[7]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip8/failure_pct", desc = "This chip's PEC failure rate over the current window as a percentage (0 - 100).", "{=u8}", failure_pct[8]);
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Accumulator/PerChip/Chip9/failure_pct", desc = "This chip's PEC failure rate over the current window as a percentage (0 - 100).", "{=u8}", failure_pct[9]);

        // timing diagnostics
        let timing = diagnostics.timing();
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Timing/period", desc = "The difference in time between the most recent Service cycle, and the Service cycle before that. In ms. Will be 0ms if fewer than two Service cycles have run yet.", "{=u64}", match timing.period() { Some(duration) => duration.as_millis(), None => 0 });
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Timing/max_period", desc = "The maximum period the Service has observed while running. In ms.", "{=u64}", timing.max_period().as_millis());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Timing/work", desc = "How long the “work” of the Service took during the most recent Service cycle. In us.", "{=u64}", timing.work().as_micros());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Timing/max_work", desc = "The maximum work the Service has observed while running. In us.", "{=u64}", timing.max_work().as_micros());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Timing/lock_wait", desc = "How long the Service waited to acquire the mutex during the most recent cycle. In us.", "{=u64}", timing.lock_wait().as_micros());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Timing/max_lock_wait", desc = "The maximum lock_wait the Service has observed while running. In us.", "{=u64}", timing.max_lock_wait().as_micros());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Timing/service_frequency", desc = "The configured service frequency. This represents how long the Service waits after a cycle to wake up an run again. This is a const value!", "{=u64}", timing.service_frequency());

        // chip state diagnostics
        let chipstate = diagnostics.chip_state_diagnostics();
        /// macro for the chipstate diagnostics since i don't want to copy paste this. the only parameter is the index of the chip
        macro_rules! chipstate_diagnostics {
            ($val:literal) => {
                let state: adbms6830b::turnkey::api::ChipState = chipstate.chip_state()[$val];
                let line: adbms6830b::turnkey::api::LineId = chipstate.chip_line()[$val];
                let command_count = state.command_count();
                defmt_monitor::monitor!(["Segments/ServiceDiagnostics/ChipState/Chip", $val, "/line"], desc = "Which Line this chip is on.", "{}", line);
                defmt_monitor::monitor!(["Segments/ServiceDiagnostics/ChipState/Chip", $val, "/pec_failed_count"], desc = "Total number of times this chip has read in a failed PEC. (different to what the accumulator reports, since this is overall)", "{=usize}", state.pec_failed_count());
                defmt_monitor::monitor!(["Segments/ServiceDiagnostics/ChipState/Chip", $val, "/pec_success_count"], desc = "Total number of times this chip has read in a successful PEC. (different to what the accumulator reports, since this is overall)", "{=usize}", state.pec_success_count());
                defmt_monitor::monitor!(["Segments/ServiceDiagnostics/ChipState/Chip", $val, "/command_count_resets"], desc = "Number of times the command counter for this chip has been reset due to a sleep.", "{=usize}", state.command_count_resets());
                defmt_monitor::monitor!(["Segments/ServiceDiagnostics/ChipState/Chip", $val, "/last_contacted"], desc = "Last time we heard from this chip with a good PEC. In ms since system boot. 0ms if we have never heard from this chip.", "{=u64}", match state.last_contacted() { Some(instant) => instant.as_millis(), None => 0 });
                defmt_monitor::monitor!(["Segments/ServiceDiagnostics/ChipState/Chip", $val, "/CommandCount/expected"], desc = "What this chip’s counter “should” be. This is tracked from the commands sent to it.", "{=u8}", command_count.expected());
                defmt_monitor::monitor!(["Segments/ServiceDiagnostics/ChipState/Chip", $val, "/CommandCount/reported"], desc = "What this chip reported on the last read of it that passed its PEC.", "{=u8}", command_count.reported());
                defmt_monitor::monitor!(["Segments/ServiceDiagnostics/ChipState/Chip", $val, "/CommandCount/in_sync"], desc = "Whether the reported counter matches the expected one. This can be expected to be false in some cases, like after isoSPI recovers from a break.", "{=bool}", command_count.in_sync());
            };
        }
        chipstate_diagnostics!(0);
        chipstate_diagnostics!(1);
        chipstate_diagnostics!(2);
        chipstate_diagnostics!(3);
        chipstate_diagnostics!(4);
        chipstate_diagnostics!(5);
        chipstate_diagnostics!(6);
        chipstate_diagnostics!(7);
        chipstate_diagnostics!(8);
        chipstate_diagnostics!(9);

        // general service diagnostics
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/sleep_detection_spi_error_count", desc = "Number of times sleep detection has failed due to a SPI::Error.", "{=usize}", diagnostics.sleep_detection_spi_error_count());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/break_detection_spi_error_count", desc = "Number of times break detection has failed due to a SPI::Error.", "{=usize}", diagnostics.break_detection_spi_error_count());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/cycles_count", desc = "Number of times the service has ran so far. This increments on every loop the service makes.", "{=usize}", diagnostics.cycles_count());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/split", desc = "The current split of chips between the isoSPI lines. (you can also see the per-chip line reports if that's easier to read)", "{}", diagnostics.split());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/max_split_attempts", desc = "The configured maximum split attempts for isoSPI recovery. This is a const value!", "{=usize}", diagnostics.max_split_attempts());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/max_verification_attempts", desc = "The configured maximum verification attempts for isoSPI recovery. This is a const value!", "{=usize}", diagnostics.max_verification_attempts());

        // line diagnostics
        let line = diagnostics.line_diagnostics();
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Line/line_a_error_count", desc = "Total number of times Line A has failed with a SPI::Error.", "{=usize}", line.line_a_error_count());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Line/most_recent_line_a_error", desc = "Most recent Error that has occured on Line A. None if no errors have occured yet. (this is for HAL-level errors, and has nothing to do with PEC errors or anything like that)", "{}", line.most_recent_line_a_error());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Line/line_b_error_count", desc = "Total number of times Line B has failed with a SPI::Error.", "{=usize}", line.line_b_error_count());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Line/most_recent_line_b_error", desc = "Most recent Error that has occured on Line B. None if no errors have occured yet. (this is for HAL-level errors, and has nothing to do with PEC errors or anything like that)", "{}", line.most_recent_line_b_error());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Line/line_a_chips_detected_count", desc = "Current chips detected as REACHABLE on Line A. Not at all linked to the official line split. Can be affected by PEC noise. Will report an error if detection failed this service cycle.", "{}", line.line_a_chips_detected_count());
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Line/line_b_chips_detected_count", desc = "Current chips detected as REACHABLE on Line B. Not at all linked to the official line split. Can be affected by PEC noise. Will report an error if detection failed this service cycle.", "{}", line.line_b_chips_detected_count());

        // how long the logging in this closure took
        defmt_monitor::monitor!("Segments/ServiceDiagnostics/Timing/diagnostics_overhead", desc = "How long the ServiceDiagnostics logging took. In micros", "{=u64}", embassy_time::Instant::now().saturating_duration_since(diagnostics_started).as_micros());
    },

    // ADBMS6830B Service startup sequence! this gets called by the service at boot time, and whenever the service needs to restart the chips (isospi recovery or sleep detection)
    async |api, _reason| {
        use adbms6830b::chip::registers::{
            config_a::{
                ConfigA,
                types::{ReferenceOn, ComparisonThresholdVoltage, SoakTimeOn, SoakTimeRange, OpenWireSoakTimeMultiplier, GpioPullDownConfig, IirFilterConfig}
            },
            config_b::{
                ConfigB,
                types::{OvervoltageThreshold, UndervoltageThreshold, DischargeTimerMonitor, DischargeTimerStatus, DischargeTimerRange, DischargeCellConfig}
            }
        };
        use adbms6830b::chip::{
            commands,
            commands::adc::{
                AdcvRedundancy, Acquisition, ResetFilter, OpenWire
            }
        };
        use adbms6830b::turnkey::service::StartupResult;

        // Reset chips to blank state.
        if let Err(err) = api.reset().await {
            defmt::error!("Segments: Failed to call `api.reset()` during ADBMS6830B Service startup. Error: {}", err.to_kind());
            return StartupResult::Incomplete;
        }

        // Set up ConfigA.
        let config_a = const { 
            ConfigA::new()
            .with_refon(ReferenceOn::On)
            .with_cth(ComparisonThresholdVoltage::Mv25_05)
            // not going to do `clear_diagnostic_flags()` like the C code since they are all cleared via ConfigA::new() and if were to manually re-clear them here it would have to be 8 separate calls for each flag
            .with_soakon(SoakTimeOn::On)
            .with_owrng(SoakTimeRange::Short)
            .with_owa(OpenWireSoakTimeMultiplier::X1)
            .with_fc(IirFilterConfig::Hz10)

            // ConfigA: GPIO Pull-down settings
            .with_gpio1(GpioPullDownConfig::PullDownOff)
            .with_gpio2(GpioPullDownConfig::PullDownOff)
            .with_gpio3(GpioPullDownConfig::PullDownOff)
            .with_gpio4(GpioPullDownConfig::PullDownOff)
            .with_gpio5(GpioPullDownConfig::PullDownOff)
            .with_gpio6(GpioPullDownConfig::PullDownOff)
            .with_gpio7(GpioPullDownConfig::PullDownOff)  // this is an on board therm for beta only
            .with_gpio8(GpioPullDownConfig::PullDownOff)  // this is a on board therm
            
            // set outputs, 9=iso led 10=bal LED. false=lit up
            .with_gpio9(GpioPullDownConfig::PullDownOff)
            .with_gpio10(GpioPullDownConfig::PullDownOff)
        };
        match api.set_configa(&[config_a; alias::ADBMS6830B_NUM_CHIPS]).await {
            Ok(_) => (),
            Err(err) => {
                defmt::error!("Segments: Failed to write ConfigA during ADBMS6830B Service startup. Error: {}", err);
                return StartupResult::Incomplete;
            }
        }

        // Set up ConfigB.
        let config_b = const {
            /// VOV setting from microvolts.
            const VOV: OvervoltageThreshold = const {
                const MICROVOLTS: i32 = 4_200_000; // 4.2 volts
                OvervoltageThreshold::from_microvolts(MICROVOLTS).expect("Invalid OvervoltageThreshold for VOV.")
            };

            /// VUV setting from microvolts.
            const VUV: UndervoltageThreshold = const {
                const MICROVOLTS: i32 = 2_500_000; // 2.5 volts
                UndervoltageThreshold::from_microvolts(MICROVOLTS).expect("Invalid OvervoltageThreshold for VUV.")
            };

            ConfigB::new()
            .with_vov(VOV)
            .with_vuv(VUV)
            .with_dtmen(DischargeTimerMonitor::Disabled)
            .with_dcto(DischargeTimerStatus::new().with_increments(0))
            .with_dtrng(DischargeTimerRange::ShortRange)
            .with_dcc1(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc2(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc3(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc4(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc5(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc6(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc7(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc8(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc9(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc10(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc11(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc12(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc13(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc14(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc15(DischargeCellConfig::ShortingSwitchOff)
            .with_dcc16(DischargeCellConfig::ShortingSwitchOff)
        };
        match api.set_configb(&[config_b; alias::ADBMS6830B_NUM_CHIPS]).await {
            Ok(_) => (),
            Err(err) => {
                defmt::error!("Segments: Failed to write ConfigB during ADBMS6830B Service startup. Error: {}", err);
                return StartupResult::Incomplete;
            }
        }

        // Disable balancing on init.
        if let Err(err) = api.command(commands::discharge::mute()).await {
            defmt::error!("Segments: Failed to send `mute()` command to disable balancing during ADBMS6830B Service startup. Error = {}", err.to_kind());
            return StartupResult::Incomplete;
        }

        // Start the ADCV conversions.
        if let Err(err) = api.command(commands::adc::adcv(
                AdcvRedundancy::Enabled, 
                Acquisition::Continuous, 
                ResetFilter::Reset, 
                OpenWire::OffForAll)).await {
            defmt::error!("Segments: Failed to send command to start adcv() conversions during ADBMS6830B Service startup. Error = {}", err.to_kind());
            return StartupResult::Incomplete;
        }

        // okay startup is complete now
        // we have to delay after init is successful for 500ms to wait for ADC to start up (this is what the C code does)
        Timer::after_millis(500).await;
        StartupResult::Complete
    }
    ).await;
}
