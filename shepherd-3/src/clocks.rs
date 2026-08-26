//! Clock config stuff.

/// Clock tree config. This is meant to be the same as TSECU-Shepherd's `SystemClock_Config()` and `PeriphCommonClock_Config()`.
pub fn rcc_config() -> embassy_stm32::Config {
    use embassy_stm32::rcc::*;
    use embassy_stm32::time::Hertz;

    // notes:
    // - HSE 25 MHz crystal -> PLL1 (M=2, N=28, P/Q/R=2) -> 175 MHz SYSCLK
    // - AHB/APB1/APB2/APB3 all /1, so HCLK and every PCLK are 175 MHz
    // - HSE -> PLL2 (M=5, N=64, P/Q=5, R=2) -> 64 MHz on both P and Q
    // - FDCAN kernel clock from PLL2Q, SPI1/2 kernel clocks from PLL2P

    let mut config = embassy_stm32::Config::default();

    // 25 MHz crystal on PH0/PH1.
    config.rcc.hse = Some(Hse {
        freq: Hertz(25_000_000),
        mode: HseMode::Oscillator,
    });

    // PLL1: 25 / 2 = 12.5 MHz ref, x28 = 350 MHz VCO, /2 = 175 MHz.
    config.rcc.pll1 = Some(Pll {
        source: PllSource::HSE,
        prediv: PllPreDiv::DIV2,
        mul: PllMul::MUL28,
        divp: Some(PllDiv::DIV2),
        divq: Some(PllDiv::DIV2),
        divr: Some(PllDiv::DIV2),
    });

    // PLL2: 25 / 5 = 5 MHz ref, x64 = 320 MHz VCO, /5 = 64 MHz on P and Q.
    config.rcc.pll2 = Some(Pll {
        source: PllSource::HSE,
        prediv: PllPreDiv::DIV5,
        mul: PllMul::MUL64,
        divp: Some(PllDiv::DIV5),
        divq: Some(PllDiv::DIV5),
        divr: Some(PllDiv::DIV2),
    });

    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV1;
    config.rcc.apb2_pre = APBPrescaler::DIV1;
    config.rcc.apb3_pre = APBPrescaler::DIV1;

    // TSECU uses PWR_REGULATOR_VOLTAGE_SCALE1 (max 200 MHz).
    config.rcc.voltage_scale = VoltageScale::Scale1;

    // Kernel clock muxes. these should match PeriphCommonClock_Config() from TSECU-Shepherd.
    config.rcc.mux.fdcan12sel = mux::Fdcansel::PLL2_Q;
    config.rcc.mux.spi1sel = mux::Spi1sel::PLL2_P;
    config.rcc.mux.spi2sel = mux::Spi2sel::PLL2_P;

    config
}