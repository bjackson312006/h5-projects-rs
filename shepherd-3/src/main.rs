#![no_std]
#![no_main]

use defmt::{Debug2Format, info, warn};
use embassy_executor::Spawner;
use embassy_stm32::Config;
use embassy_stm32::bind_interrupts;
use embassy_stm32::{dma, gpio, peripherals, spi, time::mhz, Peri};
use embassy_time::{Delay, Timer};
use embedded_hal_async::spi::SpiDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use {defmt_rtt as _, panic_probe as _};

mod segments;

use assign_resources::assign_resources;
assign_resources! {
    /// Resources for segment IsoSPI Line A.
    segment_isospi_linea: SegmentIsoSpiLineAResources {
        // Line A SPI config
        linea_spi: SPI1,
        linea_sck: PA5,
        linea_mosi: PD7,
        linea_miso: PA6,
        linea_cs: PG10,
        linea_tx_dma: GPDMA1_CH0,
        linea_rx_dma: GPDMA1_CH1,
    }
    /// Resources for segment IsoSPI Line B.
    segment_isospi_lineb: SegmentIsoSpiLineBResources {
        // Line B SPI config
        lineb_spi: SPI2,
        lineb_sck: PD3,
        lineb_mosi: PG1,
        lineb_miso: PC2,
        lineb_cs: PA3,
        lineb_tx_dma: GPDMA1_CH2,
        lineb_rx_dma: GPDMA1_CH3,
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!("Initializing project...");

    let p = embassy_stm32::init(Config::default());

    let r = split_resources!(p);

    spawner.spawn(segments::segments_task(r.segment_isospi_linea, r.segment_isospi_lineb).expect("Failed to spawn segments::manager_task()."));
}
