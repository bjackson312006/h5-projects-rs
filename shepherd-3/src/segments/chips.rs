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

    pub fn from_fn(mut f: impl FnMut(ChipId) -> T) -> Self {
        Self { data: core::array::from_fn(|i| f(ChipId::VARIANTS[i])) }
    }

    pub fn iter(&self) -> Iter<'_, T> {
        ChipId::VARIANTS.iter().copied().zip(self.data.iter())
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        ChipId::VARIANTS.iter().copied().zip(self.data.iter_mut())
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