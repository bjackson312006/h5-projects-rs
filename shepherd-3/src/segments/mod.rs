//! Subsystem for managing the two isoSPI busses connecting to the ADBMS6830B chips on each segment.
//! 
//! There are 5 segments, each of which have two ADBMS6830B chips on them. So, there are 10 ADBMS6830B chips in total.

use lines::{Line, LineId, Lines, LinesInitError, SpiError};
use chips::{ChipData, ChipId, Chips};
use adbms6830b::{
    chip::registers::{WritableGroup, ReadableGroup},
    spi::{MAX_CHIPS, Error},
};

/// Private internal helper module for the Manager.
mod lines {
    use embedded_hal_bus::spi::ExclusiveDevice;
    use embassy_time::Delay;
    use embassy_stm32::{
        mode::Async,
        gpio::Output,
        spi::{ Spi, mode::Master },
    };
    use super::chips::ChipId;
    use split::Split;

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
    pub type SpiDevice = ExclusiveDevice<Spi<'static, Async, Master>, Output<'static>, Delay>;

    /// The error type our `SpiDevice` produces.
    pub type SpiError = <SpiDevice as embedded_hal_async::spi::ErrorType>::Error;

    /// Type alias representing an IsoSPI Line.
    pub type Line = adbms6830b::spi::Line<SpiDevice>;

    /// Errors that may occur when initializing `Lines`.
    #[derive(defmt::Format)]
    pub enum LinesInitError {
        /// This error means that initializing a `Line` via a `adbms6830b::spi::Line::new()` call failed.
        DriverInitError(adbms6830b::spi::InitError),
    }

    pub(in super) mod split {
        use super::{ChipId, LineId};

        pub struct Counts {
            pub(in crate::segments::lines::split) line_a: usize,
            pub(in crate::segments::lines::split) line_b: usize
        }
        impl Counts {
            /// Number of chips on Line A.
            pub fn line_a(&self) -> usize { self.line_a }
            /// Number of chips on Line B.
            pub fn line_b(&self) -> usize { self.line_b }

        }

        /// Represents a raw index on a Line. Basically this is literally
        /// just an index from the POV of the Line.
        /// 
        /// This type can only be created by `lineindex_from_chipid()`. It is purely
        /// just a structuring type for the return value of `lineindex_from_chipid()`.
        /// Because of that, there's no constructor and should not ever be!
        pub struct LineIndex {
            /// The Line the index is on.
            line: LineId,
            /// The index.
            ///
            /// At this point the `ChipId` doesn't represent
            /// a logical ChipId, it is simply the chip of
            /// the chip from the POV of the line.
            index: ChipId,
        }
        impl LineIndex {
            /// The Line this index refers to.
            pub const fn line(&self) -> LineId { self.line }
            /// The position along this LineIndex's Line (as a raw index).
            pub const fn index(&self) -> usize { self.index as usize }
        }

        pub struct Split {
            split: Option<ChipId>,
        }
        impl Split {
            /// Creates a new split.
            /// 
            /// `split` is the Chip at which the Lines are split.
            /// 
            /// `None` means that all chips are on Line A.
            /// For `Some(chip)`, `chip` is the first chip at which Line B starts.
            /// Examples:
            /// - None: There is no split. All chips are on Line A.
            /// - Some(ChipId::Chip9): The is a split. Line B starts at Chip 9. So, Chips 0 through 8 are on Line A, and Chip 9 is on Line B.
            /// - Some(ChipId::Chip5): There is a split. Line B starts at Chip 5. So, Chips 0 through 4 are on Line A, and Chips 5 through 9 are on Line B.
            /// - Some(ChipId::Chip0): There is a split. Line B starts at Chip 0. So, Chips 0 through 9 (all chips) are on Line B.
            pub const fn new(split: Option<ChipId>) -> Self {
                Self { split }
            }

            /// Returns the number of chips on each line.
            pub const fn counts(&self) -> Counts {
                match self.split {
                    None => {
                        // None means that all chips are on Line A
                        Counts {
                            line_a: ChipId::VARIANT_COUNT,
                            line_b: 0
                        }
                    },
                    Some(chip) => {
                        // `chip` is the chip where Line B starts
                        let line_a = chip as usize;
                        let line_b = ChipId::VARIANT_COUNT - line_a;
                        Counts { line_a, line_b }
                    },
                }
            }

            /// Returns what line a Chip is on based on the current split.
            pub fn line(&self, id: ChipId) -> LineId {
                match self.split {
                    None => {
                        // if split is None, then all chips are on Line A. So chipid must be on line A as well
                        LineId::LineA
                    },
                    Some(chip) => {
                        // `chip` is the chip where Line B starts
                        // so if `id` is before `chip`, `id` is on Line A. If `id` is equal to `chip` or is after `chip`, `id` is on Line B
                        if id < chip {
                            LineId::LineA
                        } else {
                            LineId::LineB
                        }
                    }
                }
            }

            /// Converts a logical ChipId into a LineIndex based on the current split.
            pub fn lineindex_from_chipid(&self, id: ChipId) -> LineIndex {
                match self.line(id) {
                    LineId::LineA => {
                        LineIndex {
                            line: LineId::LineA,
                            index: id,
                        }
                    },
                    LineId::LineB => {
                        LineIndex {
                            line: LineId::LineB,
                            index: id.reverse(),
                        }
                    },
                }
            }

            /// Converts a LineIndex to a logical ChipId based on the current split.
            pub fn chipid_from_lineindex(&self, idx: LineIndex) -> ChipId {
                let line = idx.line;
                let raw_idx = idx.index;
                match line {
                    LineId::LineA => {
                        raw_idx
                    },
                    LineId::LineB => {
                        raw_idx.reverse()
                    }
                }
            }
        }
    }

    /// Struct holding our two isoSPI lines.
    /// 
    /// This struct basically only exists to serve the larger segment manager,
    /// and streamline access to the two lines so you can only access them via
    /// the `LineId` enum. As such, only one instance of this struct is ever
    /// supposed to be created.
    pub(in crate::segments) struct Lines {
        line_a: Line,
        line_b: Line,

        /// private guy that manages where the split is set.
        split: Split,
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

        /// Gets a read-only reference to `Split`
        pub(in crate::segments) fn split(&self) -> &Split { &self.split }

        /// Initializes our two isoSPI lines.
        /// 
        /// ### Parameters
        /// - `r_linea`: pins and other hardware resources for Line A
        /// - `r_lineb`: pins and other hardware resources for Line B
        /// - `chips`: the manager's `Chips` instance (this is what's used to derive the initial number of chips on each line)
        #[function_name::named]
        pub(in crate::segments) fn init(r_linea: crate::SegmentIsoSpiLineAResources, r_lineb: crate::SegmentIsoSpiLineBResources) -> Result<Self, LinesInitError> {
            use embedded_hal_bus::spi::ExclusiveDevice;
            use embassy_time::{Delay};

            let split = split::Split::new(None);
            let counts = split.counts();

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

            let line_a  = match adbms6830b::spi::Line::new(linea_spi, counts.line_a()) {
                Ok(line_a) => line_a,
                Err(err) => { 
                    defmt::error!("In {}(): Call to `adbms6830b::spi::Line::new()` failed when trying to create `line_a`: {}", function_name!(), err);
                    return Err(LinesInitError::DriverInitError(err)); 
                }
            };

            let line_b = match adbms6830b::spi::Line::new(lineb_spi, counts.line_b()) {
                Ok(line_b) => line_b,
                Err(err) => {
                    defmt::error!("In {}(): Call to `adbms6830b::spi::Line::new()` failed when trying to create `line_b`: {}", function_name!(), err);
                    return Err(LinesInitError::DriverInitError(err)); 
                }
            };

            Ok(Self { line_a, line_b, split })
        }
    }

    /// ID for each line.
    #[derive(Copy, Clone, PartialEq)]
    #[derive(defmt::Format)]
    pub enum LineId {
        /// Corresponds to `Lines::line_a`.
        LineA,
        /// Corresponds to `Lines::line_b`.
        LineB,
    }


}

/// Private internal helper module for the Manager.
mod chips {

    /// Enum representing the 10 ADBMS6830B chips.
    #[derive(variant_count::VariantCount)]
    #[derive(Copy, Clone, Eq, PartialEq, PartialOrd)]
    #[derive(defmt::Format)]
    #[repr(usize)]
    pub(in super) enum ChipId {
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
    impl ChipId {
        /// Array of every chip in logical order.
        pub(in super) const LIST: [ChipId; ChipId::VARIANT_COUNT] = [
            ChipId::Chip0, ChipId::Chip1, ChipId::Chip2, ChipId::Chip3, ChipId::Chip4,
            ChipId::Chip5, ChipId::Chip6, ChipId::Chip7, ChipId::Chip8, ChipId::Chip9,
        ];

        /// Reverses a ChipId. This is used when converting between Line A
        /// and Line B but should probably never ever be used outside
        /// of that. This couldn't just be a simple subtraction
        /// because that would lose the type safety (would need to operate
        /// as usize which would require handling an invalid request as
        /// a runtime error)
        pub(in super) const fn reverse(&self) -> Self {
            match self {
                ChipId::Chip0 => ChipId::Chip9,
                ChipId::Chip1 => ChipId::Chip8,
                ChipId::Chip2 => ChipId::Chip7,
                ChipId::Chip3 => ChipId::Chip6,
                ChipId::Chip4 => ChipId::Chip5,
                ChipId::Chip5 => ChipId::Chip4,
                ChipId::Chip6 => ChipId::Chip3,
                ChipId::Chip7 => ChipId::Chip2,
                ChipId::Chip8 => ChipId::Chip1,
                ChipId::Chip9 => ChipId::Chip0,
            }
        }

    }

    /// List of all the ADBMS6830B chips on the segments.
    pub(in crate::segments) struct Chips {
        /// List of each Chip, containing its data.
        chips: [ChipData; ChipId::VARIANT_COUNT],
    }
    impl Chips {
        /// Initializes a `Chips` list. Right now, all chips start on Line A.
        #[function_name::named]
        pub(in crate::segments) fn init() -> Self {
            Self {
                chips: [ChipData { nonthing: 0 }; ChipId::VARIANT_COUNT]
            }
        }
        /// Gets a reference to a specific chip and its data.
        pub(in crate::segments) const fn get(&self, chip: ChipId) -> &ChipData {
            &self.chips[chip as usize]
        }
        /// Gets a mut reference to a specific chip and its data.
        pub(in crate::segments) const fn get_mut(&mut self, chip: ChipId) -> &mut ChipData {
            &mut self.chips[chip as usize]
        }

        /// Iterates over every chip in logical order.
        ///
        /// This exists so callers can say `for (id, data) in chips.iter()` instead of
        /// enumerating and casting an index back into a `ChipId`.
        pub(in crate::segments) fn iter(&self) -> impl Iterator<Item = (ChipId, &ChipData)> + '_ {
            ChipId::LIST.into_iter().map(|id| (id, &self.chips[id as usize]))
        }

        /// Like `iter()` but with mutable references to each chip's data.
        pub(in crate::segments) fn iter_mut(&mut self) -> impl Iterator<Item = (ChipId, &mut ChipData)> + '_ {
            self.chips.iter_mut().enumerate().map(|(i, data)| (ChipId::LIST[i], data))
        }
    }
    impl IntoIterator for Chips {
        type Item = ChipData;
        type IntoIter = core::array::IntoIter<ChipData, { ChipId::VARIANT_COUNT }>;

        fn into_iter(self) -> Self::IntoIter {
            self.chips.into_iter()
        }
    }

    /// Data for a Chip.
    #[derive(Copy, Clone)]
    pub(in crate::segments) struct ChipData {
        // nothing yet
        pub(in crate::segments) nonthing: u8,
    }
}

/// Per-chip responses when reading segments.
#[derive(Copy, Clone)]
pub enum ChipResponse<G> {
    /// Normal response.
    /// 
    /// This chip responded to the read as asked with no issues. The inner
    /// contains the read data for this chip.
    Okay(G),
    /// This specific chip's PEC check failed.
    PecFailed,
    /// The entire line belonging to this chip was not able to be
    /// communicated with. This is not a chip-specific issue, since it applies to all
    /// other chips that share this line. The SPI error for this chip's line, and the line ID of
    /// the failing line, can be found in the inner.
    LineFailed(Error<SpiError>, LineId),
}
impl<G> ChipResponse<G> {
    /// Returns `true` if this `ChipResponse` is `Okay`.
    pub fn is_okay(&self) -> bool {
        matches!(*self, ChipResponse::Okay(_))
    }

    /// Returns `true` if this `ChipResponse` is `PecFailed`.
    pub fn is_pecfailed(&self) -> bool {
        matches!(*self, ChipResponse::PecFailed)
    }

    /// Returns `true` if this `ChipResponse` is `LineFailed`.
    pub fn is_linefailed(&self) -> bool {
        matches!(*self, ChipResponse::LineFailed(..))
    }

    /// Returns `true` if this `ChipResponse` is `PecFailed` OR `LineFailed`.
    pub fn is_failed(&self) -> bool {
        matches!(*self, ChipResponse::LineFailed(..) | ChipResponse::PecFailed)
    }
}

/// Response when reading the segments.
pub struct Responses<G> {
    chips: [ChipResponse<G>; ChipId::VARIANT_COUNT],
}
impl<G: ReadableGroup> Responses<G> {
    /// Lets you access the response of a particular chip.
    /// 
    /// If `chip`'s PEC failed, you will get `None` here.
    pub fn device(&self, chip: ChipId) -> ChipResponse<G> { self.chips[chip as usize] }

    /// Lets you iterate over the chips whose PEC checks failed.
    pub fn pec_failures(&self) -> impl Iterator<Item = ChipId> + '_ {
        ChipId::LIST.into_iter().filter(|&chip| self.chips[chip as usize].is_pecfailed())
    }

    /// Lets you iterate over the chips whose `Line` was not able to be communicated with.
    pub fn line_failures(&self) -> impl Iterator<Item = ChipId> + '_ {
        ChipId::LIST.into_iter().filter(|&chip| self.chips[chip as usize].is_linefailed())
    }

    /// Returns true if reads from all chips were successful.
    pub fn all_ok(&self) -> bool {
        self.chips.iter().all(ChipResponse::is_okay)
    }

    /// Lets you iterate over the returned data per chip.
    pub fn iter(&self) -> impl Iterator<Item = (ChipId, ChipResponse<G>)> + '_ {
        ChipId::LIST.into_iter().map(|chip| (chip, self.chips[chip as usize]))
    }
}


/// Errors that may occur when initializing the segment manager.
#[derive(defmt::Format)]
pub enum ManagerInitError {
    /// Error that occured when trying to call `lines::Lines::init()`.
    LinesInitError(LinesInitError),
}

/// Segment manager.
struct Manager {
    /// Line A and Line B.
    lines: Lines,
    
    /// The 10 ADBMS6830B chips that we can
    /// communicate with over the isoSPI lines.
    /// Each chip starts on Line A, but can move
    /// over to Line B if needed at runtime.
    chips: Chips,
}

impl Manager {
    /// u_TODO: some kind of partition function that writes a comm break, splits the `Chips` and their lines accordingly, and then updates the number of chips on each line in `Lines`.
    /// i kinda want this to be monolithic so this is basically the only place the "number of chips on a line" state gets updated at runtime (which will hopefully make it impossible for any
    /// state mismatch between the chips array and the lines' chip counts to happen)

    /// Writes to the ADBMS6830B chips.
    /// 
    /// ### Parameters
    /// - `chips`: An array of the group data you want to write to the chips. This array is indexed in logical `ChipId`
    /// order. It automatically handles the IsoSPI line splitting.
    pub async fn write<G: WritableGroup>(&mut self, chips: &[G; ChipId::VARIANT_COUNT]) -> Result<(), Error<SpiError>> {
        let counts = self.lines.split().counts();

        let mut buf_a = *chips;
        let mut buf_b = *chips;
        for id in ChipId::LIST {
            let index = self.lines.split().lineindex_from_chipid(id);
            let buf = match index.line() {
                LineId::LineA => &mut buf_a,
                LineId::LineB => &mut buf_b,
            };
            buf[index.index()] = chips[id as usize];
        }

        let line_a = if counts.line_a() > 0 {
            self.lines.get_mut(LineId::LineA).write(&buf_a[..counts.line_a()]).await
        } else {
            Ok(())
        };

        let line_b = if counts.line_b() > 0 {
            self.lines.get_mut(LineId::LineB).write(&buf_b[..counts.line_b()]).await
        } else {
            Ok(())
        };

        line_a.and(line_b)
    }

    /// Reads a register group from every chip.
    pub async fn read<G: ReadableGroup>(&mut self) -> Responses<G> {
        let line_a = self.lines.get_mut(LineId::LineA).read_all::<G>().await;
        let line_b = self.lines.get_mut(LineId::LineB).read_all::<G>().await;

        Responses {
            chips: ChipId::LIST.map(|chip_id| {
                let index =
                    self.lines.split().lineindex_from_chipid(chip_id);

                let line_response = match index.line() {
                    LineId::LineA => &line_a,
                    LineId::LineB => &line_b,
                };

                match line_response {
                    Ok(line_response) => match line_response.device(index.index()) {
                        Some(data) => ChipResponse::Okay(data),
                        None => ChipResponse::PecFailed,
                    },
                    Err(err) => ChipResponse::LineFailed(*err, index.line()),
                }
            }),
        }
    }

    #[function_name::named]
    pub async fn init(r_linea: crate::SegmentIsoSpiLineAResources, r_lineb: crate::SegmentIsoSpiLineBResources) -> Result<Self, ManagerInitError> {

        let chips = chips::Chips::init();

        let lines = match lines::Lines::init(r_linea, r_lineb) {
            Ok(lines) => lines,
            Err(err) => {
                defmt::error!("In {}(): Call to `lines::Lines::init()` failed: {}", function_name!(), err);
                return Err(ManagerInitError::LinesInitError(err)); 
            }
        };

        Ok(Self { lines, chips })
    }
}

/// # Debug
/// 
/// Extra `Manager` methods that provide debugging data.
impl Manager {
    /// Returns what `Line` each chip currently belongs to according to `Manager`'s line split state.
    pub fn dbg_linesplit(&self) -> [LineId; ChipId::VARIANT_COUNT] {
        ChipId::LIST.map(|chip_id| self.lines.split().line(chip_id))
    }
}

#[embassy_executor::task]
#[function_name::named]
pub async fn manager_task(r_linea: crate::SegmentIsoSpiLineAResources, r_lineb: crate::SegmentIsoSpiLineBResources) {
    use embassy_time::{Duration, Timer};
    
    let manager = match Manager::init(r_linea, r_lineb).await {
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