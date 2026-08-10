//! Subsystem for managing the two isoSPI busses connecting to the ADBMS6830B chips on each segment.
//! 
//! There are 5 segments, each of which have two ADBMS6830B chips on them. So, there are 10 ADBMS6830B chips in total.

use embedded_hal_async::spi::SpiDevice;
use embassy_stm32::spi::Spi;
use embassy_stm32::mode::Async;
use embassy_stm32::spi::mode::Master;

embassy_stm32::bind_interrupts!(struct Irqs {
    GPDMA1_CHANNEL0 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH0>;
    GPDMA1_CHANNEL1 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH1>;
    GPDMA1_CHANNEL2 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH2>;
    GPDMA1_CHANNEL3 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH3>;
});

struct SegmentIsospiManager {
    line_a: adbms6830b::spi::Line<Spi<'static, Async, Master>>,
    line_b: adbms6830b::spi::Line<Spi<'static, Async, Master>>,
}

impl SegmentIsospiManager {
    pub fn init(r: crate::SegmentIsospiResources) -> Result<(), ()> {
        use embedded_hal_bus::spi::ExclusiveDevice;
        use embassy_time::{Delay, Timer};

        let mut spi_config = embassy_stm32::spi::Config::default();
        spi_config.frequency = embassy_stm32::time::mhz(1);

        let linea_spi = embassy_stm32::spi::Spi::new(
            r.linea_spi,
            r.linea_sck,
            r.linea_mosi,
            r.linea_miso,
            r.linea_tx_dma,
            r.linea_rx_dma,
            Irqs,
            spi_config,
        );
        let linea_cs = embassy_stm32::gpio::Output::new(r.linea_cs, embassy_stm32::gpio::Level::High, embassy_stm32::gpio::Speed::High);
        let linea_spi = ExclusiveDevice::new(linea_spi, linea_cs, Delay).unwrap();

        let lineb_spi = embassy_stm32::spi::Spi::new(
            r.lineb_spi,
            r.lineb_sck,
            r.lineb_mosi,
            r.lineb_miso,
            r.lineb_tx_dma,
            r.lineb_rx_dma,
            Irqs,
            spi_config,
        );

        let line_a = match adbms6830b::spi::Line::new(linea_spi, 10) {
            Ok(line_a) => line_a,
            Err(_) => { return Err(()); }
        };

        Self {
            line_a: adbms6830b::spi::Line::new(linea_spi, 10)
        }
    }
}