//! Module for caching SPI reads to the ADBMS6830B chips.

use adbms6830b::{chip::registers::{
    ReadableGroup,
    pwm::{PwmA, PwmB},
    results::{RedundantAuxillaryA, RedundantAuxillaryB, RedundantAuxillaryC, RedundantAuxillaryD},
    results::{CellVoltagesA, CellVoltagesB, CellVoltagesC, CellVoltagesD, CellVoltagesE},
    results::{AverageCellVoltagesA, AverageCellVoltagesB, AverageCellVoltagesC, AverageCellVoltagesD, AverageCellVoltagesE},
    results::{FilteredCellVoltagesA, FilteredCellVoltagesB, FilteredCellVoltagesC, FilteredCellVoltagesD, FilteredCellVoltagesE},
    results::{SVoltagesA, SVoltagesB, SVoltagesC, SVoltagesD, SVoltagesE},
    status::{StatusC,},
}, turnkey::api::LineId};
use adbms6830b::line::Error;
use crate::segments::core::alias::{SpiError, Service};
use adbms6830b::line::PecStatus;
use super::chips::ChipId;
use core::cell::Cell;
use super::chips::IndexByChip;
use super::core::alias;
use super::chips::ADBMS6830B_NUM_CHIPS;

/// Cache to hold read data.
pub(super) static CACHE: CacheData = CacheData::new();

/// Errors that may occur when trying to update a value in the cache.
#[derive(Clone, Copy, Debug)]
#[derive(defmt::Format)]
pub enum UpdateError {
    /// Error occurred while trying to clear flags after reading them in an update call.
    ClearFlagsError(Error<SpiError>),
    /// Error occurred while trying to send the UNSNAP command.
    UnsnapError(Error<SpiError>),
    /// Error occurred while trying to send the SNAP command.
    SnapError(Error<SpiError>),
    /// Error occurred while polling a conversion completion (possibly via a ...autoconvert() function).
    PollError(Error<SpiError>),
    /// Line A failed during update. Inner contains the SPI error.
    LineAFailed(Error<SpiError>),
    /// Line B failed during update. Inner contains the SPI error.
    LineBFailed(Error<SpiError>),
    /// Both lines failed during update. Inner contains both SPI errors.
    BothLinesFailed { linea_err: Error<SpiError>, lineb_err: Error<SpiError> },
    /// Impossible error that should not be possible to happen. Using this instead of unreachable!()
    /// or an unwrap/expect so an impossible error that somehow happens doesn't panic the whole bms
    ImpossibleError,
}

/// Register reading for a single chip.
#[derive(Copy, Clone, Debug)]
pub struct Reading<R: ReadableGroup> {
    data: R,
    pec: PecStatus,
}
impl<R: ReadableGroup> Reading<R> {
    /// Actual register reading.
    pub const fn data(&self) -> R { self.data }
    /// The PEC status of the reading.
    pub const fn pec(&self) -> PecStatus { self.pec }
}

/// Actual register cache data (held inside blocking mutex)
#[derive(Copy, Clone)]
pub struct RegisterCacheData<R: ReadableGroup> {
    /// Contains the read data for each chip. Starts out as `None` if this register hasn't been cached yet.
    data: Option<IndexByChip<Reading<R>>>,
    /// Last instant this register cache was successfully read over SPI and updated.
    /// If no read has been made yet, this is None.
    last_sucessful_read: Option<embassy_time::Instant>,
}
// ^^ u_TODO ideas for maybe cool extra stuff we could add to RegisterCacheData: 
// - a `read_duration: Option<embassy_time::Duration>` that stores how long the most recent read took. maybe also a `max_read_duration`, `min_read_duration`, and `avg_read_duration` field? would actually be helpful for optimizing our timing a bit. or just cool to look at
// - idk

impl<R: ReadableGroup> RegisterCacheData<R> {
    /// Last instant this register cache was successfully read over SPI and updated.
    /// If no read has been made yet, this is None.
    pub const fn last_sucessful_read(&self) -> Option<embassy_time::Instant> {
        self.last_sucessful_read
    }

    /// Register read data for each chip.
    /// If no read has been made yet, this is None.
    pub const fn data(&self) -> &Option<IndexByChip<Reading<R>>> {
        &self.data
    }
}


pub struct RegisterCache<R: ReadableGroup> {
    inner: embassy_sync::blocking_mutex::ThreadModeMutex<Cell<RegisterCacheData<R>>>,
}

impl<R: ReadableGroup> RegisterCache<R> {
    /// New uninitialized register cache.
    pub const fn new() -> Self {
        Self {
            inner: embassy_sync::blocking_mutex::ThreadModeMutex::new(Cell::new(RegisterCacheData {
                data: None,
                last_sucessful_read: None,
            }))
        }
    }

    /// Copies out Register Cache data. Copy is needed here due to the mutex, since multiple threads read the cache. Hopefully compiler uses RVO?
    pub fn data(&self) -> RegisterCacheData<R> {
        self.inner.lock(|inner| {
            inner.get()
        })
    }

    /// Reads the register and updates the cache.
    pub async fn update(&self, api: &mut alias::Api) -> Result<(), UpdateError> {
        use strum::EnumCount;
        use super::chips::ChipId;

        let data: [Reading<R>; ADBMS6830B_NUM_CHIPS] = {
            let responses = api.read::<R>().await;

            match (responses.line_error(LineId::A), responses.line_error(LineId::B)) {
                // Both lines failed.
                (Some(linea_err), Some(lineb_err)) => {
                    defmt::error!("Segments: cache: In RegisterCache::update(): SPI Read on both Line A and Line B failed. Errors: linea_err={}, lineb_err={}", linea_err, lineb_err);
                    return Err(UpdateError::BothLinesFailed{ linea_err: *linea_err, lineb_err: *lineb_err });
                },

                // Line A failed, but not Line B.
                (Some(linea_err), None) => {
                    defmt::error!("Segments: cache: In RegisterCache::update(): SPI Read on Line A failed. Error: {}", linea_err);
                    return Err(UpdateError::LineAFailed(*linea_err));
                },

                // Line B failed, but not Line A.
                (None, Some(lineb_err)) => {
                    defmt::error!("Segments: cache: In RegisterCache::update(): SPI Read on Line B failed. Error: {}", lineb_err);
                    return Err(UpdateError::LineBFailed(*lineb_err));
                },

                // Neither line failed so we're good
                (None, None) => (),
            }

            let readings: [Reading<R>; ADBMS6830B_NUM_CHIPS] = {
                let Some(readings) = responses.iter().map(|response| {
                    response.map(|response| 
                        Reading {
                            data: response.data(),
                            pec: response.pec(),
                        }
                    )})
                    .collect::<Option<heapless::Vec<Reading<R>, { ADBMS6830B_NUM_CHIPS }>>>()
                    .and_then(|readings| readings.into_array::<{ ADBMS6830B_NUM_CHIPS }>().ok())
                else {
                    // u_Note: there is probably a way to restructure this so that ImpossibleError doesn't need to exist at all, but it might require going into the driver which is kinda annoying. so even though this existing is kinda gross it is probably fine for now
                    defmt::error!("Segments: cache: In RegisterCache::update(): a chip reading was `None` even though we already verified that no line errors occured. This should not be possible.");
                    return Err(UpdateError::ImpossibleError);
                };

                readings
            };

            readings
        };

        self.inner.lock(|inner| {
            inner.set(RegisterCacheData {
                data: Some(IndexByChip::new(data)),
                last_sucessful_read: Some(embassy_time::Instant::now()),
            });
        });

        Ok(())
    }
}

pub struct CacheData {
    raxa: RegisterCache<RedundantAuxillaryA>,
    raxb: RegisterCache<RedundantAuxillaryB>,
    raxc: RegisterCache<RedundantAuxillaryC>,
    raxd: RegisterCache<RedundantAuxillaryD>,

    cva: RegisterCache<CellVoltagesA>,
    cvb: RegisterCache<CellVoltagesB>,
    cvc: RegisterCache<CellVoltagesC>,
    cvd: RegisterCache<CellVoltagesD>,
    cve: RegisterCache<CellVoltagesE>,

    aca: RegisterCache<AverageCellVoltagesA>,
    acb: RegisterCache<AverageCellVoltagesB>,
    acc: RegisterCache<AverageCellVoltagesC>,
    acd: RegisterCache<AverageCellVoltagesD>,
    ace: RegisterCache<AverageCellVoltagesE>,

    fca: RegisterCache<FilteredCellVoltagesA>,
    fcb: RegisterCache<FilteredCellVoltagesB>,
    fcc: RegisterCache<FilteredCellVoltagesC>,
    fcd: RegisterCache<FilteredCellVoltagesD>,
    fce: RegisterCache<FilteredCellVoltagesE>,
    
    sca: RegisterCache<SVoltagesA>,
    scb: RegisterCache<SVoltagesB>,
    scc: RegisterCache<SVoltagesC>,
    scd: RegisterCache<SVoltagesD>,
    sce: RegisterCache<SVoltagesE>,

    statc: RegisterCache<StatusC>,
}
impl CacheData {
    pub(super) const fn new() -> Self {
        Self {
            raxa: RegisterCache::new(),
            raxb: RegisterCache::new(),
            raxc: RegisterCache::new(),
            raxd: RegisterCache::new(),

            cva: RegisterCache::new(),
            cvb: RegisterCache::new(),
            cvc: RegisterCache::new(),
            cvd: RegisterCache::new(),
            cve: RegisterCache::new(),

            aca: RegisterCache::new(),
            acb: RegisterCache::new(),
            acc: RegisterCache::new(),
            acd: RegisterCache::new(),
            ace: RegisterCache::new(),

            fca: RegisterCache::new(),
            fcb: RegisterCache::new(),
            fcc: RegisterCache::new(),
            fcd: RegisterCache::new(),
            fce: RegisterCache::new(),

            sca: RegisterCache::new(),
            scb: RegisterCache::new(),
            scc: RegisterCache::new(),
            scd: RegisterCache::new(),
            sce: RegisterCache::new(),

             statc: RegisterCache::new(),
        }
    }
}

/// Register groups RedundantAuxillaryA through D.
pub mod redundant_aux {
    use super::*;
    use crate::units::ElectricPotential;
    use uom::si::{electric_potential::microvolt};
    use super::alias;

    /// Raw Redundant Aux register readings.
    pub struct Raw {
        pub raxa: RegisterCacheData<RedundantAuxillaryA>,
        pub raxb: RegisterCacheData<RedundantAuxillaryB>,
        pub raxc: RegisterCacheData<RedundantAuxillaryC>,
        pub raxd: RegisterCacheData<RedundantAuxillaryD>,
    }
    impl Raw {
        /// Tries to make it nice.
        pub fn try_nice(&self) -> Result<NiceData, ()> { NiceData::try_from(self) }
    }
    // ^^ note: this struct is just meant to be a nice helper for formatting returned data. the `CacheData` struct is still meant to directly hold these registers itself
    
    /// "Nice data" for a single chip.
    pub struct NiceDataChip {
        /// GPIO1 Voltage result.
        pub gpio1_votlage: ElectricPotential,
        /// GPIO2 Voltage result.
        pub gpio2_votlage: ElectricPotential,
        /// GPIO3 Voltage result.
        pub gpio3_votlage: ElectricPotential,
        /// GPIO4 Voltage result.
        pub gpio4_votlage: ElectricPotential,
        /// GPIO5 Voltage result.
        pub gpio5_votlage: ElectricPotential,
        /// GPIO6 Voltage result.
        pub gpio6_votlage: ElectricPotential,
        /// GPIO7 Voltage result.
        pub gpio7_votlage: ElectricPotential,
        /// GPIO8 Voltage result.
        pub gpio8_votlage: ElectricPotential,
        /// GPIO9 Voltage result.
        pub gpio9_votlage: ElectricPotential,
        /// GPIO10 Voltage result.
        pub gpio10_votlage: ElectricPotential,
    }

    /// Represents the raw register readings, but formatted in a more readable way.
    /// 
    /// This doesn't contain any metadata about the reading (e.g., PEC errors). So you should
    /// probably inspect that stuff from the `Raw` readings before converting to this.
    pub struct NiceData { inner: IndexByChip<NiceDataChip> }
    impl core::ops::Deref for NiceData {
        type Target = IndexByChip<NiceDataChip>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }
    impl TryFrom<&Raw> for NiceData {
        type Error = ();

        /// Attempts to convert `Raw` data into a `NiceData`. If any of the registers involved
        /// in this `NiceData` haven't been read yet, this returns `Err(())`.
        fn try_from(raw: &Raw) -> Result<Self, Self::Error> {
            let Some(a) = raw.raxa.data() else { return Err(()); };
            let Some(b) = raw.raxb.data() else { return Err(()); };
            let Some(c) = raw.raxc.data() else { return Err(()); };
            let Some(d) = raw.raxd.data() else { return Err(()); };

            Ok(Self {
                inner: {
                    IndexByChip::from_fn(|chip| {
                        NiceDataChip {
                            gpio1_votlage: ElectricPotential::new::<microvolt>(a.get(chip).data().r_g1v().as_microvolts() as f32),
                            gpio2_votlage: ElectricPotential::new::<microvolt>(a.get(chip).data().r_g2v().as_microvolts() as f32),
                            gpio3_votlage: ElectricPotential::new::<microvolt>(a.get(chip).data().r_g3v().as_microvolts() as f32),

                            gpio4_votlage: ElectricPotential::new::<microvolt>(b.get(chip).data().r_g4v().as_microvolts() as f32),
                            gpio5_votlage: ElectricPotential::new::<microvolt>(b.get(chip).data().r_g5v().as_microvolts() as f32),
                            gpio6_votlage: ElectricPotential::new::<microvolt>(b.get(chip).data().r_g6v().as_microvolts() as f32),

                            gpio7_votlage: ElectricPotential::new::<microvolt>(c.get(chip).data().r_g7v().as_microvolts() as f32),
                            gpio8_votlage: ElectricPotential::new::<microvolt>(c.get(chip).data().r_g8v().as_microvolts() as f32),
                            gpio9_votlage: ElectricPotential::new::<microvolt>(c.get(chip).data().r_g9v().as_microvolts() as f32),

                            gpio10_votlage: ElectricPotential::new::<microvolt>(d.get(chip).data().r_g10v().as_microvolts() as f32),
                        }
                    })
                }
            })
        }
    }

    impl CacheData {
        /// Updates caches RedundantAuxillaryA through D with new data.
        /// 
        /// ### Returns
        /// Will return `Ok(())`, or `Err(UpdateError)` if an error occurred. If this returns `Ok(())`, the cached data was updated correctly and can be read now.
        pub(in crate::segments) async fn update_redundant_aux(&self, api: &mut alias::Api) -> Result<(), UpdateError> {
            use adbms6830b::chip::commands::adc::Aux2InputSelection;

            /// Autoconvert timeout in ms.
            const TIMEOUT_MS: u64 = 100;

            match api.adax2_autoconvert(Aux2InputSelection::All, TIMEOUT_MS).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_redundant_aux(): call to `api.adax2_autoconvert` resulted in an error. Error: {}", err);
                    return Err(UpdateError::PollError(err));
                }
            }

            self.raxa.update(api).await?;
            self.raxb.update(api).await?;
            self.raxc.update(api).await?;
            self.raxd.update(api).await?;

            Ok(())
        }

        /// Gets the current cached Redundant Aux data.
        pub fn get_redundant_aux(&self) -> redundant_aux::Raw {
            redundant_aux::Raw {
                raxa: self.raxa.data(),
                raxb: self.raxb.data(),
                raxc: self.raxc.data(),
                raxd: self.raxd.data(),
            }
        }
    }
}

/// Register groups CellVoltages A through E (no F because we only use 13 cells).
pub mod cell_voltages {
    use super::*;
    use crate::units::ElectricPotential;
    use uom::si::{electric_potential::microvolt};
    use super::alias;
    use crate::segments::chips::cells::{IndexByCell, CellId};

    /// Raw CellVoltages register readings.
    pub struct Raw {
        pub cva: RegisterCacheData<CellVoltagesA>,
        pub cvb: RegisterCacheData<CellVoltagesB>,
        pub cvc: RegisterCacheData<CellVoltagesC>,
        pub cvd: RegisterCacheData<CellVoltagesD>,
        pub cve: RegisterCacheData<CellVoltagesE>,
    }
    impl Raw {
        /// Tries to make it nice.
        pub fn try_nice(&self) -> Result<NiceData, ()> { NiceData::try_from(self) }
    }
    // ^^ note: this struct is just meant to be a nice helper for formatting returned data. the `CacheData` struct is still meant to directly hold these registers itself
    
    /// "Nice data" for a single chip.
    pub struct NiceDataChip {
        inner: IndexByCell<ElectricPotential>,
    }
    impl core::ops::Deref for NiceDataChip {
        type Target = IndexByCell<ElectricPotential>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    /// Represents the raw register readings, but formatted in a more readable way.
    /// 
    /// This doesn't contain any metadata about the reading (e.g., PEC errors). So you should
    /// probably inspect that stuff from the `Raw` readings before converting to this.
    pub struct NiceData { inner: IndexByChip<NiceDataChip> }
    impl core::ops::Deref for NiceData {
        type Target = IndexByChip<NiceDataChip>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }
    impl TryFrom<&Raw> for NiceData {
        type Error = ();

        /// Attempts to convert `Raw` data into a `NiceData`. If any of the registers involved
        /// in this `NiceData` haven't been read yet, this returns `Err(())`.
        fn try_from(raw: &Raw) -> Result<Self, Self::Error> {
            let Some(a) = raw.cva.data() else { return Err(()); };
            let Some(b) = raw.cvb.data() else { return Err(()); };
            let Some(c) = raw.cvc.data() else { return Err(()); };
            let Some(d) = raw.cvd.data() else { return Err(()); };
            let Some(e) = raw.cve.data() else { return Err(()); };

            Ok(Self {
                inner: {
                    IndexByChip::from_fn(|chip| {
                        NiceDataChip {
                            inner: IndexByCell::from_fn(|cell| {
                                match cell {
                                    CellId::Cell1 => ElectricPotential::new::<microvolt>(a.get(chip).data().c1v().as_microvolts() as f32),
                                    CellId::Cell2 => ElectricPotential::new::<microvolt>(a.get(chip).data().c2v().as_microvolts() as f32),
                                    CellId::Cell3 => ElectricPotential::new::<microvolt>(a.get(chip).data().c3v().as_microvolts() as f32),

                                    CellId::Cell4 => ElectricPotential::new::<microvolt>(b.get(chip).data().c4v().as_microvolts() as f32),
                                    CellId::Cell5 => ElectricPotential::new::<microvolt>(b.get(chip).data().c5v().as_microvolts() as f32),
                                    CellId::Cell6 => ElectricPotential::new::<microvolt>(b.get(chip).data().c6v().as_microvolts() as f32),

                                    CellId::Cell7 => ElectricPotential::new::<microvolt>(c.get(chip).data().c7v().as_microvolts() as f32),
                                    CellId::Cell8 => ElectricPotential::new::<microvolt>(c.get(chip).data().c8v().as_microvolts() as f32),
                                    CellId::Cell9 => ElectricPotential::new::<microvolt>(c.get(chip).data().c9v().as_microvolts() as f32),

                                    CellId::Cell10 => ElectricPotential::new::<microvolt>(d.get(chip).data().c10v().as_microvolts() as f32),
                                    CellId::Cell11 => ElectricPotential::new::<microvolt>(d.get(chip).data().c11v().as_microvolts() as f32),
                                    CellId::Cell12 => ElectricPotential::new::<microvolt>(d.get(chip).data().c12v().as_microvolts() as f32),

                                    CellId::Cell13 => ElectricPotential::new::<microvolt>(e.get(chip).data().c13v().as_microvolts() as f32),
                                }
                            })
                        }
                    })
                }
            })
        }
    }

    impl CacheData {
        /// Updates caches CellVoltages A through E with new data.
        /// 
        /// ### Returns
        /// Will return `Ok(())`, or `Err(UpdateError)` if an error occurred. If this returns `Ok(())`, the cached data was updated correctly and can be read now.
        pub(in crate::segments) async fn update_cell_voltages(&self, api: &mut alias::Api) -> Result<(), UpdateError> {
            use adbms6830b::chip::commands::snapshot::{snap, unsnap};

            match api.command(snap()).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_cell_voltages(): call to `api.command(snap())` resulted in an error. Error: {}", err);
                    return Err(UpdateError::SnapError(err));
                }
            }

            let result: Result<(), UpdateError> = async {
                self.cva.update(api).await?;
                self.cvb.update(api).await?;
                self.cvc.update(api).await?;
                self.cvd.update(api).await?;
                self.cve.update(api).await?;
                Ok(())
            }.await;

            match api.command(unsnap()).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_cell_voltages(): call to `api.command(unsnap())` resulted in an error. Error: {}", err);
                    return Err(UpdateError::UnsnapError(err));
                }
            }

            result
        }

        /// Gets the current cached Cell Voltages data.
        pub fn get_cell_voltages(&self) -> cell_voltages::Raw {
            cell_voltages::Raw {
                cva: self.cva.data(),
                cvb: self.cvb.data(),
                cvc: self.cvc.data(),
                cvd: self.cvd.data(),
                cve: self.cve.data(),
            }
        }
    }
}

/// Register groups AverageCellVoltages A through E (no F because we only use 13 cells).
pub mod average_cell_voltages {
    use super::*;
    use crate::units::ElectricPotential;
    use uom::si::{electric_potential::microvolt};
    use super::alias;
    use crate::segments::chips::cells::{IndexByCell, CellId};

    /// Raw AverageCellVoltages register readings.
    pub struct Raw {
        pub aca: RegisterCacheData<AverageCellVoltagesA>,
        pub acb: RegisterCacheData<AverageCellVoltagesB>,
        pub acc: RegisterCacheData<AverageCellVoltagesC>,
        pub acd: RegisterCacheData<AverageCellVoltagesD>,
        pub ace: RegisterCacheData<AverageCellVoltagesE>,
    }
    impl Raw {
        /// Tries to make it nice.
        pub fn try_nice(&self) -> Result<NiceData, ()> { NiceData::try_from(self) }
    }
    // ^^ note: this struct is just meant to be a nice helper for formatting returned data. the `CacheData` struct is still meant to directly hold these registers itself
    
    /// "Nice data" for a single chip.
    pub struct NiceDataChip {
        inner: IndexByCell<ElectricPotential>,
    }
    impl core::ops::Deref for NiceDataChip {
        type Target = IndexByCell<ElectricPotential>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    /// Represents the raw register readings, but formatted in a more readable way.
    /// 
    /// This doesn't contain any metadata about the reading (e.g., PEC errors). So you should
    /// probably inspect that stuff from the `Raw` readings before converting to this.
    pub struct NiceData { inner: IndexByChip<NiceDataChip> }
    impl core::ops::Deref for NiceData {
        type Target = IndexByChip<NiceDataChip>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }
    impl TryFrom<&Raw> for NiceData {
        type Error = ();

        /// Attempts to convert `Raw` data into a `NiceData`. If any of the registers involved
        /// in this `NiceData` haven't been read yet, this returns `Err(())`.
        fn try_from(raw: &Raw) -> Result<Self, Self::Error> {
            let Some(a) = raw.aca.data() else { return Err(()); };
            let Some(b) = raw.acb.data() else { return Err(()); };
            let Some(c) = raw.acc.data() else { return Err(()); };
            let Some(d) = raw.acd.data() else { return Err(()); };
            let Some(e) = raw.ace.data() else { return Err(()); };

            Ok(Self {
                inner: {
                    IndexByChip::from_fn(|chip| {
                        NiceDataChip {
                            inner: IndexByCell::from_fn(|cell| {
                                match cell {
                                    CellId::Cell1 => ElectricPotential::new::<microvolt>(a.get(chip).data().ac1v().as_microvolts() as f32),
                                    CellId::Cell2 => ElectricPotential::new::<microvolt>(a.get(chip).data().ac2v().as_microvolts() as f32),
                                    CellId::Cell3 => ElectricPotential::new::<microvolt>(a.get(chip).data().ac3v().as_microvolts() as f32),

                                    CellId::Cell4 => ElectricPotential::new::<microvolt>(b.get(chip).data().ac4v().as_microvolts() as f32),
                                    CellId::Cell5 => ElectricPotential::new::<microvolt>(b.get(chip).data().ac5v().as_microvolts() as f32),
                                    CellId::Cell6 => ElectricPotential::new::<microvolt>(b.get(chip).data().ac6v().as_microvolts() as f32),

                                    CellId::Cell7 => ElectricPotential::new::<microvolt>(c.get(chip).data().ac7v().as_microvolts() as f32),
                                    CellId::Cell8 => ElectricPotential::new::<microvolt>(c.get(chip).data().ac8v().as_microvolts() as f32),
                                    CellId::Cell9 => ElectricPotential::new::<microvolt>(c.get(chip).data().ac9v().as_microvolts() as f32),

                                    CellId::Cell10 => ElectricPotential::new::<microvolt>(d.get(chip).data().ac10v().as_microvolts() as f32),
                                    CellId::Cell11 => ElectricPotential::new::<microvolt>(d.get(chip).data().ac11v().as_microvolts() as f32),
                                    CellId::Cell12 => ElectricPotential::new::<microvolt>(d.get(chip).data().ac12v().as_microvolts() as f32),

                                    CellId::Cell13 => ElectricPotential::new::<microvolt>(e.get(chip).data().ac13v().as_microvolts() as f32),
                                }
                            })
                        }
                    })
                }
            })
        }
    }

    impl CacheData {
        /// Updates caches AverageCellVoltages A through E with new data.
        /// 
        /// ### Returns
        /// Will return `Ok(())`, or `Err(UpdateError)` if an error occurred. If this returns `Ok(())`, the cached data was updated correctly and can be read now.
        pub(in crate::segments) async fn update_average_cell_voltages(&self, api: &mut alias::Api) -> Result<(), UpdateError> {
            use adbms6830b::chip::commands::snapshot::{snap, unsnap};

            match api.command(snap()).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_average_cell_voltages(): call to `api.command(snap())` resulted in an error. Error: {}", err);
                    return Err(UpdateError::SnapError(err));
                }
            }

            let result: Result<(), UpdateError> = async {
                self.aca.update(api).await?;
                self.acb.update(api).await?;
                self.acc.update(api).await?;
                self.acd.update(api).await?;
                self.ace.update(api).await?;
                Ok(())
            }.await;

            match api.command(unsnap()).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_average_cell_voltages(): call to `api.command(unsnap())` resulted in an error. Error: {}", err);
                    return Err(UpdateError::UnsnapError(err));
                }
            }

            result
        }

        /// Gets the current cached Average Cell Voltages data.
        pub fn get_average_cell_voltages(&self) -> average_cell_voltages::Raw {
            average_cell_voltages::Raw {
                aca: self.aca.data(),
                acb: self.acb.data(),
                acc: self.acc.data(),
                acd: self.acd.data(),
                ace: self.ace.data(),
            }
        }
    }
}

/// Register groups FilteredCellVoltages A through E (no F because we only use 13 cells).
pub mod filtered_cell_voltages {
    use super::*;
    use crate::units::ElectricPotential;
    use uom::si::{electric_potential::microvolt};
    use super::alias;
    use crate::segments::chips::cells::{IndexByCell, CellId};

    /// Raw FilteredCellVoltages register readings.
    pub struct Raw {
        pub fca: RegisterCacheData<FilteredCellVoltagesA>,
        pub fcb: RegisterCacheData<FilteredCellVoltagesB>,
        pub fcc: RegisterCacheData<FilteredCellVoltagesC>,
        pub fcd: RegisterCacheData<FilteredCellVoltagesD>,
        pub fce: RegisterCacheData<FilteredCellVoltagesE>,
    }
    impl Raw {
        /// Tries to make it nice.
        pub fn try_nice(&self) -> Result<NiceData, ()> { NiceData::try_from(self) }
    }
    // ^^ note: this struct is just meant to be a nice helper for formatting returned data. the `CacheData` struct is still meant to directly hold these registers itself
    
    /// "Nice data" for a single chip.
    pub struct NiceDataChip {
        inner: IndexByCell<ElectricPotential>,
    }
    impl core::ops::Deref for NiceDataChip {
        type Target = IndexByCell<ElectricPotential>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    /// Represents the raw register readings, but formatted in a more readable way.
    /// 
    /// This doesn't contain any metadata about the reading (e.g., PEC errors). So you should
    /// probably inspect that stuff from the `Raw` readings before converting to this.
    pub struct NiceData { inner: IndexByChip<NiceDataChip> }
    impl core::ops::Deref for NiceData {
        type Target = IndexByChip<NiceDataChip>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }
    impl TryFrom<&Raw> for NiceData {
        type Error = ();

        /// Attempts to convert `Raw` data into a `NiceData`. If any of the registers involved
        /// in this `NiceData` haven't been read yet, this returns `Err(())`.
        fn try_from(raw: &Raw) -> Result<Self, Self::Error> {
            let Some(a) = raw.fca.data() else { return Err(()); };
            let Some(b) = raw.fcb.data() else { return Err(()); };
            let Some(c) = raw.fcc.data() else { return Err(()); };
            let Some(d) = raw.fcd.data() else { return Err(()); };
            let Some(e) = raw.fce.data() else { return Err(()); };

            Ok(Self {
                inner: {
                    IndexByChip::from_fn(|chip| {
                        NiceDataChip {
                            inner: IndexByCell::from_fn(|cell| {
                                match cell {
                                    CellId::Cell1 => ElectricPotential::new::<microvolt>(a.get(chip).data().fc1v().as_microvolts() as f32),
                                    CellId::Cell2 => ElectricPotential::new::<microvolt>(a.get(chip).data().fc2v().as_microvolts() as f32),
                                    CellId::Cell3 => ElectricPotential::new::<microvolt>(a.get(chip).data().fc3v().as_microvolts() as f32),

                                    CellId::Cell4 => ElectricPotential::new::<microvolt>(b.get(chip).data().fc4v().as_microvolts() as f32),
                                    CellId::Cell5 => ElectricPotential::new::<microvolt>(b.get(chip).data().fc5v().as_microvolts() as f32),
                                    CellId::Cell6 => ElectricPotential::new::<microvolt>(b.get(chip).data().fc6v().as_microvolts() as f32),

                                    CellId::Cell7 => ElectricPotential::new::<microvolt>(c.get(chip).data().fc7v().as_microvolts() as f32),
                                    CellId::Cell8 => ElectricPotential::new::<microvolt>(c.get(chip).data().fc8v().as_microvolts() as f32),
                                    CellId::Cell9 => ElectricPotential::new::<microvolt>(c.get(chip).data().fc9v().as_microvolts() as f32),

                                    CellId::Cell10 => ElectricPotential::new::<microvolt>(d.get(chip).data().fc10v().as_microvolts() as f32),
                                    CellId::Cell11 => ElectricPotential::new::<microvolt>(d.get(chip).data().fc11v().as_microvolts() as f32),
                                    CellId::Cell12 => ElectricPotential::new::<microvolt>(d.get(chip).data().fc12v().as_microvolts() as f32),

                                    CellId::Cell13 => ElectricPotential::new::<microvolt>(e.get(chip).data().fc13v().as_microvolts() as f32),
                                }
                            })
                        }
                    })
                }
            })
        }
    }

    impl CacheData {
        /// Updates caches FilteredCellVoltages A through E with new data.
        /// 
        /// ### Returns
        /// Will return `Ok(())`, or `Err(UpdateError)` if an error occurred. If this returns `Ok(())`, the cached data was updated correctly and can be read now.
        pub(in crate::segments) async fn update_filtered_cell_voltages(&self, api: &mut alias::Api) -> Result<(), UpdateError> {
            use adbms6830b::chip::commands::snapshot::{snap, unsnap};

            match api.command(snap()).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_filtered_cell_voltages(): call to `api.command(snap())` resulted in an error. Error: {}", err);
                    return Err(UpdateError::SnapError(err));
                }
            }

            let result: Result<(), UpdateError> = async {
                self.fca.update(api).await?;
                self.fcb.update(api).await?;
                self.fcc.update(api).await?;
                self.fcd.update(api).await?;
                self.fce.update(api).await?;
                Ok(())
            }.await;

            match api.command(unsnap()).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_filtered_cell_voltages(): call to `api.command(unsnap())` resulted in an error. Error: {}", err);
                    return Err(UpdateError::UnsnapError(err));
                }
            }

            result
        }

        /// Gets the current cached Filtered Cell Voltages data.
        pub fn get_filtered_cell_voltages(&self) -> filtered_cell_voltages::Raw {
            filtered_cell_voltages::Raw {
                fca: self.fca.data(),
                fcb: self.fcb.data(),
                fcc: self.fcc.data(),
                fcd: self.fcd.data(),
                fce: self.fce.data(),
            }
        }
    }
}

/// Register groups SVoltages A through E (no F because we only use 13 cells).
pub mod s_voltages {
    use super::*;
    use crate::units::ElectricPotential;
    use uom::si::{electric_potential::microvolt};
    use super::alias;
    use crate::segments::chips::cells::{IndexByCell, CellId};

    /// Raw SVoltages register readings.
    pub struct Raw {
        pub sca: RegisterCacheData<SVoltagesA>,
        pub scb: RegisterCacheData<SVoltagesB>,
        pub scc: RegisterCacheData<SVoltagesC>,
        pub scd: RegisterCacheData<SVoltagesD>,
        pub sce: RegisterCacheData<SVoltagesE>,
    }
    impl Raw {
        /// Tries to make it nice.
        pub fn try_nice(&self) -> Result<NiceData, ()> { NiceData::try_from(self) }
    }
    // ^^ note: this struct is just meant to be a nice helper for formatting returned data. the `CacheData` struct is still meant to directly hold these registers itself
    
    /// "Nice data" for a single chip.
    pub struct NiceDataChip {
        inner: IndexByCell<ElectricPotential>,
    }
    impl core::ops::Deref for NiceDataChip {
        type Target = IndexByCell<ElectricPotential>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }

    /// Represents the raw register readings, but formatted in a more readable way.
    /// 
    /// This doesn't contain any metadata about the reading (e.g., PEC errors). So you should
    /// probably inspect that stuff from the `Raw` readings before converting to this.
    pub struct NiceData { inner: IndexByChip<NiceDataChip> }
    impl core::ops::Deref for NiceData {
        type Target = IndexByChip<NiceDataChip>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }
    impl TryFrom<&Raw> for NiceData {
        type Error = ();

        /// Attempts to convert `Raw` data into a `NiceData`. If any of the registers involved
        /// in this `NiceData` haven't been read yet, this returns `Err(())`.
        fn try_from(raw: &Raw) -> Result<Self, Self::Error> {
            let Some(a) = raw.sca.data() else { return Err(()); };
            let Some(b) = raw.scb.data() else { return Err(()); };
            let Some(c) = raw.scc.data() else { return Err(()); };
            let Some(d) = raw.scd.data() else { return Err(()); };
            let Some(e) = raw.sce.data() else { return Err(()); };

            Ok(Self {
                inner: {
                    IndexByChip::from_fn(|chip| {
                        NiceDataChip {
                            inner: IndexByCell::from_fn(|cell| {
                                match cell {
                                    CellId::Cell1 => ElectricPotential::new::<microvolt>(a.get(chip).data().s1v().as_microvolts() as f32),
                                    CellId::Cell2 => ElectricPotential::new::<microvolt>(a.get(chip).data().s2v().as_microvolts() as f32),
                                    CellId::Cell3 => ElectricPotential::new::<microvolt>(a.get(chip).data().s3v().as_microvolts() as f32),

                                    CellId::Cell4 => ElectricPotential::new::<microvolt>(b.get(chip).data().s4v().as_microvolts() as f32),
                                    CellId::Cell5 => ElectricPotential::new::<microvolt>(b.get(chip).data().s5v().as_microvolts() as f32),
                                    CellId::Cell6 => ElectricPotential::new::<microvolt>(b.get(chip).data().s6v().as_microvolts() as f32),

                                    CellId::Cell7 => ElectricPotential::new::<microvolt>(c.get(chip).data().s7v().as_microvolts() as f32),
                                    CellId::Cell8 => ElectricPotential::new::<microvolt>(c.get(chip).data().s8v().as_microvolts() as f32),
                                    CellId::Cell9 => ElectricPotential::new::<microvolt>(c.get(chip).data().s9v().as_microvolts() as f32),

                                    CellId::Cell10 => ElectricPotential::new::<microvolt>(d.get(chip).data().s10v().as_microvolts() as f32),
                                    CellId::Cell11 => ElectricPotential::new::<microvolt>(d.get(chip).data().s11v().as_microvolts() as f32),
                                    CellId::Cell12 => ElectricPotential::new::<microvolt>(d.get(chip).data().s12v().as_microvolts() as f32),

                                    CellId::Cell13 => ElectricPotential::new::<microvolt>(e.get(chip).data().s13v().as_microvolts() as f32),
                                }
                            })
                        }
                    })
                }
            })
        }
    }

    impl CacheData {
        /// Updates caches SVoltages A through E with new data.
        /// 
        /// ### Returns
        /// Will return `Ok(())`, or `Err(UpdateError)` if an error occurred. If this returns `Ok(())`, the cached data was updated correctly and can be read now.
        pub(in crate::segments) async fn update_s_voltages(&self, api: &mut alias::Api) -> Result<(), UpdateError> {
            use adbms6830b::chip::commands::snapshot::{snap, unsnap};

            match api.command(snap()).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_s_voltages(): call to `api.command(snap())` resulted in an error. Error: {}", err);
                    return Err(UpdateError::SnapError(err));
                }
            }

            let result: Result<(), UpdateError> = async {
                self.sca.update(api).await?;
                self.scb.update(api).await?;
                self.scc.update(api).await?;
                self.scd.update(api).await?;
                self.sce.update(api).await?;
                Ok(())
            }.await;

            match api.command(unsnap()).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_s_voltages(): call to `api.command(unsnap())` resulted in an error. Error: {}", err);
                    return Err(UpdateError::UnsnapError(err));
                }
            }

            result
        }

        /// Gets the current cached S Voltages data.
        pub fn get_s_voltages(&self) -> s_voltages::Raw {
            s_voltages::Raw {
                sca: self.sca.data(),
                scb: self.scb.data(),
                scc: self.scc.data(),
                scd: self.scd.data(),
                sce: self.sce.data(),
            }
        }
    }
}

/// Status C register group.
pub mod status_c {
    use super::*;
    use super::alias;
    use crate::segments::chips::cells::{IndexByCell, CellId};
    use adbms6830b::chip::registers::status::types::c::{ComparisonFault, ConversionsCount, 
        STrimMultipleError, STrimError, CTrimMultipleError, CTrimError, DigitalRailOvervoltage, DigitalRailUndervoltage, 
        AnalogRailUndervoltage, AnalogRailOvervoltage, OscillatorCheck, TestModeDetection, ThermalShutdownStatus, SleepModeDetection, 
        SpiFault, ComparisonActive, SupplyRailDelta, SupplyRailDeltaLatent};

    /// Raw StatusC register reading.
    pub struct Raw {
        pub statc: RegisterCacheData<StatusC>,
    }
    // ^^ note: this struct is just meant to be a nice helper for formatting returned data. the `CacheData` struct is still meant to directly hold these registers itself
    
    /// "Nice data" for a single chip.
    /// 
    /// This data isn't even that "nice", since it is mostly just the raw StatusC data but with nicer ComparisonFault formatting (can be indexed by cells) and nicer
    /// conversions count formatting. Everything else is basically the same though.
    pub struct NiceDataChip {
        pub cell_channel_comparison_faults: IndexByCell<ComparisonFault>,
        pub conversions_count: ConversionsCount,
        pub s_trim_multiple_error: STrimMultipleError,
        pub s_trim_error: STrimError,
        pub c_trim_multiple_error: CTrimMultipleError,
        pub c_trim_error: CTrimError,
        pub digital_rail_undervoltage: DigitalRailUndervoltage,
        pub digital_rail_overvoltage: DigitalRailOvervoltage,
        pub analog_rail_undervoltage: AnalogRailUndervoltage,
        pub analog_rail_overvoltage: AnalogRailOvervoltage,
        pub oscillator_check: OscillatorCheck,
        pub test_mode_detection: TestModeDetection,
        pub thermal_shutdown_status: ThermalShutdownStatus,
        pub sleep_mode_detection: SleepModeDetection,
        pub spi_fault: SpiFault,
        pub comparison_active: ComparisonActive,
        pub supply_rail_delta: SupplyRailDelta,
        pub supply_rail_delta_latent: SupplyRailDeltaLatent,
    }
    impl Raw {
        /// Tries to make it nice.
        pub fn try_nice(&self) -> Result<NiceData, ()> { NiceData::try_from(self) }
    }

    /// Represents the raw register readings, but formatted in a more readable way.
    /// 
    /// This doesn't contain any metadata about the reading (e.g., PEC errors). So you should
    /// probably inspect that stuff from the `Raw` readings before converting to this.
    pub struct NiceData { inner: IndexByChip<NiceDataChip> }
    impl core::ops::Deref for NiceData {
        type Target = IndexByChip<NiceDataChip>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }
    impl TryFrom<&Raw> for NiceData {
        type Error = ();

        /// Attempts to convert `Raw` data into a `NiceData`. If any of the registers involved
        /// in this `NiceData` haven't been read yet, this returns `Err(())`.
        fn try_from(raw: &Raw) -> Result<Self, Self::Error> {
            let Some(statc) = raw.statc.data() else { return Err(()); };

            Ok(Self {
                inner: {
                    IndexByChip::from_fn(|chip| {
                        let chip_statc = statc.get(chip).data();
                        NiceDataChip {
                            cell_channel_comparison_faults: IndexByCell::from_fn(|cell| {
                                match cell {
                                    CellId::Cell1 => chip_statc.cs1flt(),
                                    CellId::Cell2 => chip_statc.cs2flt(),
                                    CellId::Cell3 => chip_statc.cs3flt(),
                                    CellId::Cell4 => chip_statc.cs4flt(),
                                    CellId::Cell5 => chip_statc.cs5flt(),
                                    CellId::Cell6 => chip_statc.cs6flt(),
                                    CellId::Cell7 => chip_statc.cs7flt(),
                                    CellId::Cell8 => chip_statc.cs8flt(),
                                    CellId::Cell9 => chip_statc.cs9flt(),
                                    CellId::Cell10 => chip_statc.cs10flt(),
                                    CellId::Cell11 => chip_statc.cs11flt(),
                                    CellId::Cell12 => chip_statc.cs12flt(),
                                    CellId::Cell13 => chip_statc.cs13flt(),
                                }
                            }),
                            conversions_count: ConversionsCount::new(chip_statc.ct_lower(), chip_statc.ct_upper(), chip_statc.cts()),
                            s_trim_multiple_error: chip_statc.smed(),
                            s_trim_error: chip_statc.sed(),
                            c_trim_multiple_error: chip_statc.cmed(),
                            c_trim_error: chip_statc.ced(),
                            digital_rail_overvoltage: chip_statc.vd_ov(),
                            digital_rail_undervoltage: chip_statc.vd_uv(),
                            analog_rail_overvoltage: chip_statc.va_ov(),
                            analog_rail_undervoltage: chip_statc.va_uv(),
                            oscillator_check: chip_statc.oscchk(),
                            test_mode_detection: chip_statc.tmodchk(),
                            thermal_shutdown_status: chip_statc.thsd(),
                            sleep_mode_detection: chip_statc.sleep(),
                            spi_fault: chip_statc.spiflt(),
                            comparison_active: chip_statc.comp(),
                            supply_rail_delta: chip_statc.vde(),
                            supply_rail_delta_latent: chip_statc.vdel(),
                        }
                    })
                }
            })
        }
    }

    impl CacheData {
        /// Updates StatusC cache.
        /// 
        /// ### Returns
        /// Will return `Ok(())`, or `Err(UpdateError)` if an error occurred. If this returns `Ok(())`, the cached data was updated correctly and can be read now.
        pub(in crate::segments) async fn update_status_c(&self, api: &mut alias::Api) -> Result<(), UpdateError> {
            use adbms6830b::chip::commands::snapshot::{snap, unsnap};
            use adbms6830b::chip::registers::clear::{ClearFlags, types::ClearAction};

            match api.command(snap()).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_status_c(): call to `api.command(snap())` resulted in an error. Error: {}", err);
                    return Err(UpdateError::SnapError(err));
                }
            }

            let result: Result<(), UpdateError> = async {
                self.statc.update(api).await?;
                Ok(())
            }.await;

            match api.command(unsnap()).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_status_c(): call to `api.command(unsnap())` resulted in an error. Error: {}", err);
                    return Err(UpdateError::UnsnapError(err));
                }
            }

            match api.write(&[ClearFlags::clear_all(); ADBMS6830B_NUM_CHIPS]).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_status_c(): call to `api.write(...)` for ClearFlags resulted in an error. Error: {}", err);
                    return Err(UpdateError::ClearFlagsError(err));
                }
            }

            result
        }

        /// Gets the current cached StatusC data.
        pub fn get_status_c(&self) -> status_c::Raw {
            status_c::Raw {
                statc: self.statc.data(),
            }
        }
    }
}

// u_TODO - IMPORTANT: probably should move the SNAP calls outside of these individual `update...` functions and make it the job of the Jobs in the segments task. There should probably be a single Job for all SNAP-able register groups that snaps once, does all of the reads, and then unsnaps. because rn each individual job does its own snap which works fine within each job but data from across jobs won't be 100% coherent and it probably needs to be