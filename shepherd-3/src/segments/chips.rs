/// ID for each ADBMS6830 chip.
#[repr(usize)]
#[derive(variant_count::VariantCount)]
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
    /// Every `ChipId`, in discriminant order, so `ALL[i as usize] == i`.
    ///
    /// Keep this in the same order as the variants above. Indexing (e.g. `IndexByChip`)
    /// relies on a chip's position here matching its `as usize` cast.
    pub const ALL: [ChipId; ChipId::VARIANT_COUNT] = [
        ChipId::Chip0,
        ChipId::Chip1,
        ChipId::Chip2,
        ChipId::Chip3,
        ChipId::Chip4,
        ChipId::Chip5,
        ChipId::Chip6,
        ChipId::Chip7,
        ChipId::Chip8,
        ChipId::Chip9,
    ];

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
pub struct IndexByChip<const N: usize, T> { data: [T; N] }
pub type Iter<'a, T> = core::iter::Zip<core::array::IntoIter<ChipId, { super::alias::ADBMS6830B_NUM_CHIPS }>, core::slice::Iter<'a, T>>;
pub type IterMut<'a, T> = core::iter::Zip<core::array::IntoIter<ChipId, { super::alias::ADBMS6830B_NUM_CHIPS }>, core::slice::IterMut<'a, T>>;
pub type IntoIter<const N: usize, T> = core::iter::Zip<core::array::IntoIter<ChipId, { super::alias::ADBMS6830B_NUM_CHIPS }>, core::array::IntoIter<T, N>>;

impl<const N: usize, T> IndexByChip<N, T> {
    /// Creates a new `IndexByChip` directly from an array.
    pub const fn new(data: [T; N]) -> Self {
        Self { data }
    }

    /// Compile-time checker that makes sure N is the same size as the number of chips we have.
    const N_CHECK: () = assert!(
        N == super::alias::ADBMS6830B_NUM_CHIPS,
        "IndexByChip's N must equal the number of ChipId variants",
    );

    /// Retrives the data for `chip`.
    pub const fn get(&self, chip: ChipId) -> &T {
        let i: usize = chip as usize;
        &self.data[i]
    }

    pub fn from_fn(mut f: impl FnMut(ChipId) -> T) -> Self {
        let () = Self::N_CHECK;
        Self { data: core::array::from_fn(|i| f(ChipId::ALL[i])) }
    }

    pub fn iter(&self) -> Iter<'_, T> {
        let () = Self::N_CHECK;
        ChipId::ALL.into_iter().zip(self.data.iter())
    }

    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        let () = Self::N_CHECK;
        ChipId::ALL.into_iter().zip(self.data.iter_mut())
    }
}

impl<const N: usize, T> IntoIterator for IndexByChip<N, T> {
    type Item = (ChipId, T);
    type IntoIter = IntoIter<N, T>;

    fn into_iter(self) -> Self::IntoIter {
        let () = Self::N_CHECK;
        ChipId::ALL.into_iter().zip(self.data)
    }
}

impl<'a, const N: usize, T> IntoIterator for &'a IndexByChip<N, T> {
    type Item = (ChipId, &'a T);
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter { self.iter() }
}

impl<'a, const N: usize, T> IntoIterator for &'a mut IndexByChip<N, T> {
    type Item = (ChipId, &'a mut T);
    type IntoIter = IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter { self.iter_mut() }
}