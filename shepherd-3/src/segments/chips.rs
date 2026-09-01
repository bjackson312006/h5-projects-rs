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