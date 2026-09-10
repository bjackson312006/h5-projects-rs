use strum::{VariantArray, EnumCount};

use super::core::alias;

pub mod cells {
    use strum::{VariantArray, EnumCount};

    /// How many cells are on each chip in our setup.
    pub const ADBMS6830B_NUM_CELLS_PER_CHIP: usize = CellId::COUNT;

    /// ID for each cell per ADBMS6830B chip. There are 13 cells per chip.
    #[repr(usize)]
    #[derive(strum::EnumCount, strum::VariantArray)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[derive(defmt::Format)]
    pub enum CellId {
        Cell1,
        Cell2,
        Cell3,
        Cell4,
        Cell5,
        Cell6,
        Cell7,
        Cell8,
        Cell9,
        Cell10,
        Cell11,
        Cell12,
        Cell13,
    }

    /// Like IndexByChip but for cells
    #[derive(Copy, Clone, Debug)]
    pub struct IndexByCell<T> { data: [T; ADBMS6830B_NUM_CELLS_PER_CHIP] }
    pub type CellIds = core::iter::Copied<core::slice::Iter<'static, CellId>>;
    pub type Iter<'borrow, T> = core::iter::Zip<CellIds, core::slice::Iter<'borrow, T>>;
    pub type IterMut<'borrow, T> = core::iter::Zip<CellIds, core::slice::IterMut<'borrow, T>>;
    pub type IntoIter<T> = core::iter::Zip<CellIds, core::array::IntoIter<T, { ADBMS6830B_NUM_CELLS_PER_CHIP }>>;

    impl<T> IndexByCell<T> {
        /// Creates a new `IndexByCell` directly from an array.
        pub const fn new(data: [T; ADBMS6830B_NUM_CELLS_PER_CHIP]) -> Self {
            Self { data }
        }

        /// Retrives the data for `cell`.
        pub const fn get(&self, cell: CellId) -> &T {
            let i: usize = cell as usize;
            &self.data[i]
        }

        pub fn from_fn(mut f: impl FnMut(CellId) -> T) -> Self {
            Self { data: core::array::from_fn(|i| f(CellId::VARIANTS[i])) }
        }

        pub fn iter(&self) -> Iter<'_, T> {
            CellId::VARIANTS.iter().copied().zip(self.data.iter())
        }

        pub fn iter_mut(&mut self) -> IterMut<'_, T> {
            CellId::VARIANTS.iter().copied().zip(self.data.iter_mut())
        }

        /// Converts this back into its inner array.
        pub fn into_array(self) -> [T; ADBMS6830B_NUM_CELLS_PER_CHIP] {
            self.data
        }
    }

    impl<T> IntoIterator for IndexByCell<T> {
        type Item = (CellId, T);
        type IntoIter = IntoIter<T>;

        fn into_iter(self) -> Self::IntoIter {
            CellId::VARIANTS.iter().copied().zip(self.data)
        }
    }

    impl<'borrow, T> IntoIterator for &'borrow IndexByCell<T> {
        type Item = (CellId, &'borrow T);
        type IntoIter = Iter<'borrow, T>;

        fn into_iter(self) -> Self::IntoIter { self.iter() }
    }

    impl<'borrow, T> IntoIterator for &'borrow mut IndexByCell<T> {
        type Item = (CellId, &'borrow mut T);
        type IntoIter = IterMut<'borrow, T>;

        fn into_iter(self) -> Self::IntoIter { self.iter_mut() }
    }
}

/// Number of ADBMS6830B chips we have.
/// 
/// (this is just an alais for the ChipId count, but it kind of reads better like this)
pub const ADBMS6830B_NUM_CHIPS: usize = const { ChipId::COUNT };

/// ID for each ADBMS6830 chip.
#[repr(usize)]
#[derive(strum::EnumCount, strum::VariantArray)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(defmt::Format)]
pub enum ChipId {
    /// Segment 0, Alpha Chip
    Chip0,
    /// Segment 0, Beta Chip
    Chip1,
    /// Segment 1, Alpha Chip
    Chip2,
    /// Segment 1, Beta Chip
    Chip3,
    /// Segment 2, Alpha Chip
    Chip4,
    /// Segment 2, Beta Chip
    Chip5,
    /// Segment 3, Alpha Chip
    Chip6,
    /// Segment 3, Beta Chip
    Chip7,
    /// Segment 4, Alpha Chip
    Chip8,
    /// Segment 4, Beta Chip
    Chip9
}
impl ChipId {
    /// Whether a chip is Alpha or Beta.
    pub const fn kind(&self) -> ChipKind {
        if ((*self as usize) % 2) == 0 {
            ChipKind::Alpha
        } else {
            ChipKind::Beta
        }
    }

    /// Whether or not this chip is Alpha.
    pub const fn is_alpha(&self) -> bool {
        matches!(self.kind(), ChipKind::Alpha)
    }

    /// Whether or not this chip is Beta.
    pub const fn is_beta(&self) -> bool {
        matches!(self.kind(), ChipKind::Beta)
    }

    /// Indicates what segment this chip is on.
    pub const fn segment(&self) -> SegmentId {
        match self {
            ChipId::Chip0 => SegmentId::Segment0,
            ChipId::Chip1 => SegmentId::Segment0,
            ChipId::Chip2 => SegmentId::Segment1,
            ChipId::Chip3 => SegmentId::Segment1,
            ChipId::Chip4 => SegmentId::Segment2,
            ChipId::Chip5 => SegmentId::Segment2,
            ChipId::Chip6 => SegmentId::Segment3,
            ChipId::Chip7 => SegmentId::Segment3,
            ChipId::Chip8 => SegmentId::Segment4,
            ChipId::Chip9 => SegmentId::Segment4,
        }
    }
}

/// The type of the chip (Alpha or Beta).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(variant_count::VariantCount)]
#[derive(defmt::Format)]
pub enum ChipKind {
    Alpha,
    Beta,
}

/// ID for each segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(variant_count::VariantCount)]
#[derive(defmt::Format)]
pub enum SegmentId {
    Segment0,
    Segment1,
    Segment2,
    Segment3,
    Segment4,
}

/// Small wrapper around an array of responses for each chip.
/// You can put any datatype in here for `T` as long as it makes
/// sense to index it by a ChipId.
/// 
/// The point of this so responses can be interacted with
/// via `ChipId` (and iterated over) instead of having to
/// lookup raw arrays (whuch might require you to convert a ChipId to usize).
#[derive(Copy, Clone, Debug)]
pub struct IndexByChip<T> { data: [T; ADBMS6830B_NUM_CHIPS] }
pub type ChipIds = core::iter::Copied<core::slice::Iter<'static, ChipId>>;
pub type Iter<'borrow, T> = core::iter::Zip<ChipIds, core::slice::Iter<'borrow, T>>;
pub type IterMut<'borrow, T> = core::iter::Zip<ChipIds, core::slice::IterMut<'borrow, T>>;
pub type IntoIter<T> = core::iter::Zip<ChipIds, core::array::IntoIter<T, { ADBMS6830B_NUM_CHIPS }>>;

impl<T> IndexByChip<T> {
    /// Creates a new `IndexByChip` directly from an array.
    pub const fn new(data: [T; ADBMS6830B_NUM_CHIPS]) -> Self {
        Self { data }
    }

    /// Retrives the data for `chip`.
    pub const fn get(&self, chip: ChipId) -> &T {
        let i: usize = chip as usize;
        &self.data[i]
    }

    /// Retrieves a mutable reference to the data for `chip`.
    pub const fn get_mut(&mut self, chip: ChipId) -> &mut T {
        let i: usize = chip as usize;
        &mut self.data[i]
    }

    pub fn from_fn(mut f: impl FnMut(ChipId) -> T) -> Self {
        Self { data: core::array::from_fn(|i| f(ChipId::VARIANTS[i])) }
    }

    pub fn iter(&self) -> Iter<'_, T> {
        ChipId::VARIANTS.iter().copied().zip(self.data.iter())
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        ChipId::VARIANTS.iter().copied().zip(self.data.iter_mut())
    }

    /// Converts this back into its inner array.
    pub fn into_array(self) -> [T; ADBMS6830B_NUM_CHIPS] {
        self.data
    }
}

impl<T> IntoIterator for IndexByChip<T> {
    type Item = (ChipId, T);
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        ChipId::VARIANTS.iter().copied().zip(self.data)
    }
}

impl<'borrow, T> IntoIterator for &'borrow IndexByChip<T> {
    type Item = (ChipId, &'borrow T);
    type IntoIter = Iter<'borrow, T>;

    fn into_iter(self) -> Self::IntoIter { self.iter() }
}

impl<'borrow, T> IntoIterator for &'borrow mut IndexByChip<T> {
    type Item = (ChipId, &'borrow mut T);
    type IntoIter = IterMut<'borrow, T>;

    fn into_iter(self) -> Self::IntoIter { self.iter_mut() }
}

/// Module that stores the mapping between the 10 GPIOs (see RAUX and AUX), the 13 cells, and the 7 thermistors.
pub mod gpios {
    use super::cells::{CellId, IndexByCell};
    use strum::{EnumCount, VariantArray};
    use crate::units::{Resistance, Temperature, Voltage};

    // u_Note: this mapping is based on TSECU-Shepherd. this should probably be checked as its possible i am not reading the code correctly
    // anyway, each ADBMS6830B chip has 10 gpios. at least for 25A, if my understanding is correct, the board is set up so each of these GPIOs is tied to a thermistor.
    // there are 10 thermistors total. 7 of those thermistors are used for cell temperatures (there are 13 cells, so some cells share a thermistor). the other 3 thermistors are
    // used for on-board temp.
    //
    // i think the mapping is like this:
    // GPIO1 = cells 0, 1
    // GPIO2 = cells 2, 3
    // GPIO3 = on-board temp 0
    // GPIO4 = on-board temp 1
    // GPIO5 = on-board temp 2
    // GPIO6 = cells 4, 5
    // GPIO7 = cells 6, 7
    // GPIO8 = cells 8, 9
    // GPIO9 = cells 10, 11
    // GPIO10 = just cell 12
    // (this zero-indexes the cells but the cells are 1-indexed in the enum)

    /// The 10 GPIOs (see RAUX and AUX).
    #[derive(strum::EnumCount, strum::VariantArray)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum GpioId {
        /// Thermistor for cells 1 and 2.
        Gpio1,
        /// Thermistor for cells 3 and 4.
        Gpio2,
        /// Thermistor for on board temp 1.
        Gpio3,
        /// Thermistor for on board temp 2.
        Gpio4,
        /// Thermistor for on board temp 3.
        Gpio5,
        /// Thermistor for cells 5 and 6.
        Gpio6,
        /// Thermistor for cells 7 and 8.
        Gpio7,
        /// Thermistor for cells 9 and 10.
        Gpio8,
        /// Thermistor for cells 11 and 12.
        Gpio9,
        /// Thermistor for cell 13.
        Gpio10,
    }
    pub const ADBMS6830B_NUM_GPIOS_PER_CHIP: usize = GpioId::COUNT;

    /// Calculates the cell temperature of a 10,000 ohm NTP resistor (model 103).
    /// 
    /// ### Parameters
    /// - `res`: The resistance of the resistor.
    /// 
    /// ### Returns
    /// - The temperature.
    /// 
    /// ### Notes
    /// This function was taken from the TSECU-Shepherd C code (analyzer.c).
    fn calc_temp(resistance: &Resistance) -> Temperature {
        use uom::si::electrical_resistance::ohm;
        use uom::si::thermodynamic_temperature::degree_celsius;

        let ohms = resistance.get::<ohm>();
        
        // achieved via math --  See BMS 25 Mapping and Calcs
	    let temp: f32 = ((298.15_f32 * 3462.28_f32) / (298.15_f32 * libm::logf(ohms / 10100_f32) + 3462.28_f32)) - 273.15_f32;

        Temperature::new::<degree_celsius>(temp)
    }

    /// Calculate a cell temperature based on the thermistor reading.
    /// 
    /// ### Parameters
    /// - `voltage`: The thremistor reading.
    /// 
    /// ### Returns
    /// - The temperature.
    /// 
    /// ### Notes
    /// This function was taken from the TSECU-Shepherd C code (analyzer.c).
    pub fn calc_cell_temp(voltage: &Voltage) -> Temperature {
        use uom::si::electric_potential::volt;
        use uom::si::electrical_resistance::ohm;

        let voltage: f32 = voltage.get::<volt>();

        let res: f32 = (10000_f32 * (3_f32 - voltage)) / voltage;

        let res: Resistance = Resistance::new::<ohm>(res);

        return calc_temp(&res);
    }

    /// Struct that represents the GPIO voltages, but converted into temperatures.
    /// 
    /// The layout of this struct and the temperature calculations are based on the comment near the top of this module.
    pub struct ThermistorTemperatures {
        /// Temperatures for each cell. Note that some of the temperatures will be the same between some of the cells because some of the cells share the same thermistor.
        pub cell_temperatures: IndexByCell<Temperature>,
        /// First on-board temperature.
        pub on_board_temp_1: Temperature,
        /// Second on-board temperature.
        pub on_board_temp_2: Temperature,
        /// Third on-board temperature.
        pub on_board_temp_3: Temperature,
    }
    impl From<IndexByGpio<Voltage>> for ThermistorTemperatures {
        fn from(gpios: IndexByGpio<Voltage>) -> Self {
            Self {
                cell_temperatures: IndexByCell::from_fn(|cell| {
                    match cell {
                        CellId::Cell1 => calc_cell_temp(gpios.get(GpioId::Gpio1)),
                        CellId::Cell2 => calc_cell_temp(gpios.get(GpioId::Gpio1)),

                        CellId::Cell3 => calc_cell_temp(gpios.get(GpioId::Gpio2)),
                        CellId::Cell4 => calc_cell_temp(gpios.get(GpioId::Gpio2)),

                        CellId::Cell5 => calc_cell_temp(gpios.get(GpioId::Gpio6)),
                        CellId::Cell6 => calc_cell_temp(gpios.get(GpioId::Gpio6)),

                        CellId::Cell7 => calc_cell_temp(gpios.get(GpioId::Gpio7)),
                        CellId::Cell8 => calc_cell_temp(gpios.get(GpioId::Gpio7)),

                        CellId::Cell9 => calc_cell_temp(gpios.get(GpioId::Gpio8)),
                        CellId::Cell10 => calc_cell_temp(gpios.get(GpioId::Gpio8)),

                        CellId::Cell11 => calc_cell_temp(gpios.get(GpioId::Gpio9)),
                        CellId::Cell12 => calc_cell_temp(gpios.get(GpioId::Gpio9)),

                        CellId::Cell13 => calc_cell_temp(gpios.get(GpioId::Gpio10)),
                    }
                }),
                on_board_temp_1: calc_cell_temp(gpios.get(GpioId::Gpio3)),
                on_board_temp_2: calc_cell_temp(gpios.get(GpioId::Gpio4)),
                on_board_temp_3: calc_cell_temp(gpios.get(GpioId::Gpio5)),
            }
        }
    }
    impl IndexByGpio<Voltage> {
        /// Converts GPIO voltages to temperatures.
        pub fn to_temps(&self) -> ThermistorTemperatures {
            ThermistorTemperatures::from(*self)
        }
    }

    /// Lets you index by GPIOs.
    #[derive(Copy, Clone, Debug)]
    pub struct IndexByGpio<T> { data: [T; ADBMS6830B_NUM_GPIOS_PER_CHIP] }
    pub type GpioIds = core::iter::Copied<core::slice::Iter<'static, GpioId>>;
    pub type Iter<'borrow, T> = core::iter::Zip<GpioIds, core::slice::Iter<'borrow, T>>;
    pub type IterMut<'borrow, T> = core::iter::Zip<GpioIds, core::slice::IterMut<'borrow, T>>;
    pub type IntoIter<T> = core::iter::Zip<GpioIds, core::array::IntoIter<T, { ADBMS6830B_NUM_GPIOS_PER_CHIP }>>;

    impl<T> IndexByGpio<T> {
        /// Creates a new `IndexByGpio` directly from an array.
        pub const fn new(data: [T; ADBMS6830B_NUM_GPIOS_PER_CHIP]) -> Self {
            Self { data }
        }

        /// Retrives the data for `gpio`.
        pub const fn get(&self, gpio: GpioId) -> &T {
            let i: usize = gpio as usize;
            &self.data[i]
        }

        /// Retrieves a mutable reference to the data for `gpio`.
        pub const fn get_mut(&mut self, gpio: GpioId) -> &mut T {
            let i: usize = gpio as usize;
            &mut self.data[i]
        }

        pub fn from_fn(mut f: impl FnMut(GpioId) -> T) -> Self {
            Self { data: core::array::from_fn(|i| f(GpioId::VARIANTS[i])) }
        }

        pub fn iter(&self) -> Iter<'_, T> {
            GpioId::VARIANTS.iter().copied().zip(self.data.iter())
        }

        pub fn iter_mut(&mut self) -> IterMut<'_, T> {
            GpioId::VARIANTS.iter().copied().zip(self.data.iter_mut())
        }

        /// Converts this back into its inner array.
        pub fn into_array(self) -> [T; ADBMS6830B_NUM_GPIOS_PER_CHIP] {
            self.data
        }
    }

    impl<T> IntoIterator for IndexByGpio<T> {
        type Item = (GpioId, T);
        type IntoIter = IntoIter<T>;

        fn into_iter(self) -> Self::IntoIter {
            GpioId::VARIANTS.iter().copied().zip(self.data)
        }
    }

    impl<'borrow, T> IntoIterator for &'borrow IndexByGpio<T> {
        type Item = (GpioId, &'borrow T);
        type IntoIter = Iter<'borrow, T>;

        fn into_iter(self) -> Self::IntoIter { self.iter() }
    }

    impl<'borrow, T> IntoIterator for &'borrow mut IndexByGpio<T> {
        type Item = (GpioId, &'borrow mut T);
        type IntoIter = IterMut<'borrow, T>;

        fn into_iter(self) -> Self::IntoIter { self.iter_mut() }
    }
}