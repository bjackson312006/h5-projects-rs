//! Module for caching SPI reads to the ADBMS6830B chips.

use adbms6830b::{chip::registers::{
    ReadableGroup,
    pwm::{PwmA, PwmB},
    results::{RedundantAuxillaryA, RedundantAuxillaryB, RedundantAuxillaryC, RedundantAuxillaryD},
    results::{CellVoltagesA, CellVoltagesB, CellVoltagesC, CellVoltagesD, CellVoltagesE},
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
        }
    }
}

/// Register groups RedundantAuxillaryA through D.
pub mod redundant_aux {
    use super::*;
    use crate::units::ElectricPotential;
    use uom::si::{electric_potential::microvolt, energy::Units::btu_59};
    use super::alias;

    /// Raw Redundant Aux register readings.
    pub struct Raw {
        pub raxa: RegisterCacheData<RedundantAuxillaryA>,
        pub raxb: RegisterCacheData<RedundantAuxillaryB>,
        pub raxc: RegisterCacheData<RedundantAuxillaryC>,
        pub raxd: RegisterCacheData<RedundantAuxillaryD>,
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
    impl TryFrom<Raw> for NiceData {
        type Error = ();

        /// Attempts to convert `Raw` data into a `NiceData`. If any of the registers involved
        /// in this `NiceData` haven't been read yet, this returns `Err(())`.
        fn try_from(raw: Raw) -> Result<Self, Self::Error> {
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
    impl TryFrom<Raw> for NiceData {
        type Error = ();

        /// Attempts to convert `Raw` data into a `NiceData`. If any of the registers involved
        /// in this `NiceData` haven't been read yet, this returns `Err(())`.
        fn try_from(raw: Raw) -> Result<Self, Self::Error> {
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