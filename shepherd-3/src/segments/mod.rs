//! Subsystem for managing the two isoSPI busses connecting to the ADBMS6830B chips on each segment.
//! 
//! There are 5 segments, each of which have two ADBMS6830B chips on them. So, there are 10 ADBMS6830B chips in total.

use crate::SegmentIsoSpiLineAResources;

/// Data structures for our isoSPI lines.
mod lines {
    use embedded_hal_bus::spi::ExclusiveDevice;
    use embassy_time::Delay;
    use embassy_stm32::{
        mode::Async,
        gpio::Output,
        spi::{ Spi, mode::Master },
    };
    use core::sync::atomic::{AtomicU32, Ordering};

    embassy_stm32::bind_interrupts!(struct Irqs {
        GPDMA1_CHANNEL0 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH0>;
        GPDMA1_CHANNEL1 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH1>;
        GPDMA1_CHANNEL2 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH2>;
        GPDMA1_CHANNEL3 => embassy_stm32::dma::InterruptHandler<embassy_stm32::peripherals::GPDMA1_CH3>;
    });

    /// Type alias representing a SPI controller that implements `SpiDevice` from `embedded_hal_async`.
    /// This is just a single SPI controller with a CS pin.
    /// 
    /// This is quite an ugly gross type definition due to all of the nested generics
    /// but it lets us define how we're gonna represent a SPI device in one place.
    /// Also ideally the type alias should make this type name less annoying to read.
    type SpiDevice = ExclusiveDevice<Spi<'static, Async, Master>, Output<'static>, Delay>;
    
    /// Type alias representing an IsoSPI Line.
    pub type Line = adbms6830b::spi::Line<SpiDevice>;

    /// Errors that may occur when initializing `Lines`.
    #[derive(defmt::Format)]
    pub enum LinesInitError {
        /// This error means that initializing a `Line` via a `adbms6830b::spi::Line::new()` call failed.
        DriverInitError(adbms6830b::spi::InitError),
        /// This error indicates that you've tried to crate an instance of `Lines` after one already exists.
        /// You aren't supposed to do that!
        AlreadyCreated,
    }

    /// Stores the number of instances of `Lines` that have been created.
    /// This is meant to ensure that more than one instance of `Lines`
    /// is ever created.
    static INSTANCE_COUNT: AtomicU32 = AtomicU32::new(0);

    /// Struct holding our two isoSPI lines.
    /// 
    /// This struct basically only exists to serve the larger segment manager,
    /// and streamline access to the two lines so you can only access them via
    /// the `LineId` enum. 
    /// 
    /// As such, only one instance of this struct is ever
    /// supposed to be created. This is enforced by the `INSTANCE_COUNT`
    /// tracker. This is mainly just a sanity check that should never
    /// actually trigger (because why would we try initializing multiple `Lines`)
    /// but maybe it could help catch mistakes.
    pub(in crate::segments) struct Lines {
        line_a: Line,
        line_b: Line,
    }
    impl Lines {
        /// Gets a read-only reference to a `Line`.
        pub(in crate::segments) const fn get(&self, id: LineId) -> &Line {
            match id {
                LineId::LineA => &self.line_a,
                LineId::LineB => &self.line_b,
            }
        }
        /// Gets a mutable reference to a `Line`.
        pub(in crate::segments) const fn get_mut(&mut self, id: LineId) -> &mut Line {
            match id {
                LineId::LineA => &mut self.line_a,
                LineId::LineB => &mut self.line_b,
            }
        }

        /// Initializes our two isoSPI lines.
        #[function_name::named]
        pub(in crate::segments) fn init(r_linea: crate::SegmentIsoSpiLineAResources, r_lineb: crate::SegmentIsoSpiLineBResources) -> Result<Self, LinesInitError> {
            use embedded_hal_bus::spi::ExclusiveDevice;
            use embassy_time::{Delay, Timer};

            if INSTANCE_COUNT.load(Ordering::Relaxed) >= 1 {
                defmt::error!("In {}(): Tried creating an instance of `Lines` after one already exists. You are not supposed to do that!", function_name!());
                return Err(LinesInitError::AlreadyCreated); 
            }

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
            let lineb_spi: SpiDevice = ExclusiveDevice::new(lineb_spi, lineb_cs, Delay).unwrap();

            let line_a  = match adbms6830b::spi::Line::new(linea_spi, 10) {
                Ok(line_a) => line_a,
                Err(err) => { 
                    defmt::error!("In {}(): Call to `adbms6830b::spi::Line::new()` failed when trying to create `line_a`: {}", function_name!(), err);
                    return Err(LinesInitError::DriverInitError(err)); 
                }
            };

            let line_b = match adbms6830b::spi::Line::new(lineb_spi, 0) {
                Ok(line_b) => line_b,
                Err(err) => {
                    defmt::error!("In {}(): Call to `adbms6830b::spi::Line::new()` failed when trying to create `line_b`: {}", function_name!(), err);
                    return Err(LinesInitError::DriverInitError(err)); 
                }
            };

            INSTANCE_COUNT.fetch_add(1, Ordering::Relaxed);

            Ok(Self { line_a, line_b })
        }
    }

    /// ID for each line.
    #[derive(Copy, Clone)]
    pub enum LineId {
        /// Corresponds to `Lines::line_a`.
        LineA,
        /// Corresponds to `Lines::line_b`.
        LineB,
    }


}

mod chips {
    use core::sync::atomic::{AtomicU32, Ordering};

    /// Enum representing the 10 ADBMS6830B chips.
    #[derive(variant_count::VariantCount)]
    #[derive(defmt::Format)]
    #[repr(u8)]
    pub enum ChipId {
        /// Chip 0 (Alpha chip on Segment 0).
        Chip0 = 0,
        /// Chip 1 (Beta chip on Segment 0).
        Chip1 = 1,
        /// Chip 2 (Alpha chip on Segment 1).
        Chip2 = 2,
        /// Chip 3 (Beta chip on Segment 1).
        Chip3 = 3,
        /// Chip 4 (Alpha chip on Segment 2).
        Chip4 = 4,
        /// Chip 5 (Beta chip on Segment 2).
        Chip5 = 5,
        /// Chip 6 (Alpha chip on Segment 3).
        Chip6 = 6,
        /// Chip 7 (Beta chip on Segment 3).
        Chip7 = 7,
        /// Chip 8 (Alpha chip on Segment 4).
        Chip8 = 8,
        /// Chip 9 (Beta chip on Segment 4).
        Chip9 = 9
    }

    /// Stores the number of instances of `Chips` that have been created.
    /// This is meant to ensure that more than one instance of `Chips`
    /// is ever created.
    static INSTANCE_COUNT: AtomicU32 = AtomicU32::new(0);

    /// Errors that may occur when calling `Chips::init()`.
    #[derive(defmt::Format)]
    pub enum ChipsInitError {
        /// This error indicates that you've tried to crate an instance of `Chips` after one already exists.
        /// You aren't supposed to do that!
        AlreadyCreated,
    }

    /// List of all the ADBMS6830B chips on the segments.
    pub struct Chips {
        /// List of each Chip, containing its data.
        chips: [ChipData; ChipId::VARIANT_COUNT],
    }
    impl Chips {
        /// Initializes a `Chips` list. Right now, all chips start on Line A.
        #[function_name::named]
        pub fn init() -> Result<Self, ChipsInitError> {
            if INSTANCE_COUNT.load(Ordering::Relaxed) >= 1 {
                defmt::error!("In {}(): Tried creating an instance of `Chips` after one already exists. You are not supposed to do that!", function_name!());
                return Err(ChipsInitError::AlreadyCreated); 
            }

            INSTANCE_COUNT.fetch_add(1, Ordering::Relaxed);

            Ok(Self {
                chips: [ChipData { line: super::lines::LineId::LineA }; ChipId::VARIANT_COUNT]
            })
        }
        /// Gets a reference to a specific chip and its data.
        pub const fn get(&self, chip: ChipId) -> &ChipData {
            &self.chips[chip as usize]
        }
        /// Gets a mut reference to a specific chip and its data.
        pub const fn get_mut(&mut self, chip: ChipId) -> &mut ChipData {
            &mut self.chips[chip as usize]
        }
    }
    impl IntoIterator for Chips {
        type Item = ChipData;
        type IntoIter = core::array::IntoIter<ChipData, { ChipId::VARIANT_COUNT }>;

        fn into_iter(self) -> Self::IntoIter {
            self.chips.into_iter()
        }
    }

    /// Data for a `Chip`.
    #[derive(Copy, Clone)]
    pub struct ChipData {
        /// Id of the Line that this chip is associated with.
        pub line: super::lines::LineId,
    }
}

/// Errors that may occur when initializing the segment manager.
#[derive(defmt::Format)]
pub enum ManagerInitError {
    /// Error that occured when trying to call `lines::Lines::init()`.
    LinesInitError(lines::LinesInitError),
    /// Error that occured when trying to call `chips::Chips::init()`.
    ChipsInitError(chips::ChipsInitError),
}

/// Segment manager.
struct Manager {
    /// Line A and Line B.
    lines: lines::Lines,
    
    /// The 10 ADBMS6830B chips that we can
    /// communicate with over the isoSPI lines.
    /// Each chip starts on Line A, but can move
    /// over to Line B if needed at runtime.
    chips: chips::Chips,
}

impl Manager {
    #[function_name::named]
    pub fn init(r_linea: crate::SegmentIsoSpiLineAResources, r_lineb: crate::SegmentIsoSpiLineBResources) -> Result<Self, ManagerInitError> {
        use embedded_hal_bus::spi::ExclusiveDevice;
        use embassy_time::{Delay, Timer};

        let lines = match lines::Lines::init(r_linea, r_lineb) {
            Ok(lines) => lines,
            Err(err) => {
                defmt::error!("In {}(): Call to `lines::Lines::init()` failed: {}", function_name!(), err);
                return Err(ManagerInitError::LinesInitError(err)); 
            }
        };
        let chips = match chips::Chips::init() {
            Ok(chips) => chips,
            Err(err) => {
                defmt::error!("In {}(): Call to `chips::Chips::init()` failed: {}", function_name!(), err);
                return Err(ManagerInitError::ChipsInitError(err)); 
            }
        };

        Ok(Self { lines, chips })
    }
}

#[embassy_executor::task]
#[function_name::named]
pub async fn manager_task(r_linea: crate::SegmentIsoSpiLineAResources, r_lineb: crate::SegmentIsoSpiLineBResources) {
    use embassy_time::{Duration, Timer};
    
    let manager = match Manager::init(r_linea, r_lineb) {
        Ok(manager) => manager,
        Err(err) => {
            defmt::error!("In {}(): Failed to initialize segment manager: {}", function_name!(), err);
            return; 
        }
    };

    loop {
        Timer::after_millis(500).await;
    }
}