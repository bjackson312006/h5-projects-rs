#![no_std]
#![no_main]

use adbms6830b::chip::registers::status::{StatusA, StatusB};
use adbms6830b::spi::{Chain, Error, Response};
use defmt::{Debug2Format, info, warn};
use embassy_executor::Spawner;
use embassy_stm32::Config;
use embassy_stm32::bind_interrupts;
use embassy_stm32::{dma, gpio, peripherals, spi, time::mhz};
use embassy_time::{Delay, Timer};
use embedded_hal::spi::SpiDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
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
    let spi_device = ExclusiveDevice::new(spi, spi1_cs, Delay).unwrap();
    let mut chain = Chain::<_,>::new(spi_device, 10).expect("shoot!");

    loop {
        // Read all chips' StatusB registers.
        let responses = match chain.read_all::<StatusB>() {
            Ok(response) => response,
            Err(_) => { warn!("evil error"); continue; }
        };

        // Loop through the returned responses for each chip.
        for (index, response) in responses.iter().enumerate() {
            // Check each chip for PEC errors.
            let status_b = match response {
                None => {
                    warn!("PEC error when reading chip {}!!!", index);
                    continue;
                },
                Some(status_b) => status_b,
            };
            
            // Log the data from each chip's StatusB register.
            info!("Chip {}: Digital power supply voltage: {} uV", index, status_b.vd().as_microvolts());
            info!("Chip {}: Analog power supply voltage: {} uV", index, status_b.va().as_microvolts());
            info!("Chip {}: VREF2 across resistor: {} uV", index, status_b.vres().as_microvolts());
        }

        Timer::after_millis(500).await;
    }
}
