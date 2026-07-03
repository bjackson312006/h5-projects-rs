#![no_std]
#![no_main]

use adbms6830::client::Adbms6830;
use adbms6830::client::RX_SIZE;
use adbms6830::client::SpiPollAdc;
use adbms6830::client::TX_SIZE;
use adbms6830::registers::AdbmsCommand;
use adbms6830::types::Adax;
use adbms6830::types::ConfigA;
use cortex_m::peripheral::SCB;
use cortex_m_rt::{ExceptionFrame, exception};
use defmt::debug;
use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::Config;
use embassy_stm32::bind_interrupts;
use embassy_stm32::wdg::IndependentWatchdog;
use embassy_stm32::{dma, gpio, peripherals, spi, time::mhz};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    GPDMA1_CHANNEL0 => dma::InterruptHandler<peripherals::GPDMA1_CH0>;
    GPDMA1_CHANNEL1 => dma::InterruptHandler<peripherals::GPDMA1_CH1>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    info!("Initializing project...");

    let p = embassy_stm32::init(Config::default());

    let mut spi_config = spi::Config::default();
    spi_config.frequency = mhz(1);

    let spi = spi::Spi::new(
        p.SPI1,
        p.PA5,
        p.PD7,
        p.PA6,
        p.GPDMA1_CH0,
        p.GPDMA1_CH1,
        Irqs,
        spi_config,
    );

    let spi1_cs = gpio::Output::new(p.PG10, gpio::Level::High, gpio::Speed::High);

    const IC_CNT: usize = 1;
    const CNT_TX: usize = IC_CNT * TX_SIZE;
    const CNT_RX: usize = IC_CNT * RX_SIZE;
    let mut tx_buffer = [0u8; 4 + RX_SIZE * IC_CNT];
    let mut rx_buffer = [0u8; RX_SIZE * IC_CNT];
    let mut client = Adbms6830::<_, _, _, _, IC_CNT, CNT_RX, CNT_TX>::new(
        spi,
        spi1_cs,
        SpiPollAdc::<1> {},
        embassy_time::Delay,
        &mut tx_buffer,
        &mut rx_buffer,
    );

    match client.write(&[ConfigA::default()]).await {
        Ok(_) => (),
        Err(e) => match e {
            adbms6830::client::AdbmsError::CommunicationError(_) => todo!(),
            adbms6830::client::AdbmsError::CSControlError(_) => todo!(),
            adbms6830::client::AdbmsError::PECError(_) => todo!(),
            adbms6830::client::AdbmsError::LengthMismatch { expected, got } => todo!(),
            adbms6830::client::AdbmsError::PollError => todo!(),
        },
    }

    let res = client.read::<ConfigA>().await.unwrap();
    let a = res.first().unwrap();
    a.comm_bk();

    client
        .command(Adax {
            ow: false,
            pup: false,
            ch: adbms6830::types::AdaxChannels::All,
        })
        .await
        .unwrap();

    let mut watchdog = IndependentWatchdog::new(p.IWDG, 1000000);
    watchdog.unleash();
    loop {
        debug!("Status: Alive");
        Timer::after_millis(500).await;
        watchdog.pet();
    }
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    SCB::sys_reset() // <- you could do something other than reset
}
