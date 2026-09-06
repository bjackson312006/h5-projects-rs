//! Module for caching SPI reads to the ADBMS6830B chips.

use adbms6830b::{chip::registers::{
    ReadableGroup,
    pwm::{PwmA, PwmB},
    results::{RedundantAuxillaryA, RedundantAuxillaryB, RedundantAuxillaryC, RedundantAuxillaryD},
}, turnkey::api::LineId};
use adbms6830b::line::Error;
use crate::segments::core::alias::{SpiError, Service, ADBMS6830B_NUM_CHIPS};
use adbms6830b::line::PecStatus;
use super::chips::ChipId;
use core::cell::Cell;
use super::chips::IndexByChip;
use super::core::alias;

/// Cache to hold read data.
pub(super) static CACHE: CacheData<{ ADBMS6830B_NUM_CHIPS }> = CacheData::new();

/// Errors that may occur when trying to update a value in the cache.
#[derive(Clone, Copy, Debug)]
#[derive(defmt::Format)]
pub enum UpdateError {
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
pub struct RegisterCacheData<const N: usize, R: ReadableGroup> {
    /// Contains the read data for each chip. Starts out as `None` if this register hasn't been cached yet.
    data: Option<IndexByChip<N, Reading<R>>>,
    /// Last instant this register cache was successfully read over SPI and updated.
    /// If no read has been made yet, this is None.
    last_sucessful_read: Option<embassy_time::Instant>,
}
impl<const N: usize, R: ReadableGroup> RegisterCacheData<N, R> {
    /// Last instant this register cache was successfully read over SPI and updated.
    /// If no read has been made yet, this is None.
    pub const fn last_sucessful_read(&self) -> Option<embassy_time::Instant> {
        self.last_sucessful_read
    }

    /// Register read data for each chip.
    /// If no read has been made yet, this is None.
    pub const fn data(&self) -> &Option<IndexByChip<N, Reading<R>>> {
        &self.data
    }
}


pub struct RegisterCache<const N: usize, R: ReadableGroup> {
    inner: embassy_sync::blocking_mutex::ThreadModeMutex<Cell<RegisterCacheData<N, R>>>,
}

impl<const N: usize, R: ReadableGroup> RegisterCache<N, R> {
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
    pub fn data(&self) -> RegisterCacheData<N, R> {
        self.inner.lock(|inner| {
            inner.get()
        })
    }

    /// Reads the register and updates the cache.
    pub async fn update(&self, api: &mut alias::Api) -> Result<(), UpdateError> {
        let data: [Reading<R>; N] = {
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

            let readings: [Reading<R>; N] = {
                let Some(readings) = responses.iter().map(|response| {
                    response.map(|response| 
                        Reading {
                            data: response.data(),
                            pec: response.pec(),
                        }
                    )})
                    .collect::<Option<heapless::Vec<Reading<R>, N>>>()
                    .and_then(|readings| readings.into_array::<N>().ok())
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

pub struct CacheData<const N: usize> {
    rdraxa: RegisterCache<N, RedundantAuxillaryA>,
    rdraxb: RegisterCache<N, RedundantAuxillaryB>,
    rdraxc: RegisterCache<N, RedundantAuxillaryC>,
    rdraxd: RegisterCache<N, RedundantAuxillaryD>,
}
impl<const N: usize> CacheData<N> {
    pub(super) const fn new() -> Self {
        Self {
            rdraxa: RegisterCache::new(),
            rdraxb: RegisterCache::new(),
            rdraxc: RegisterCache::new(),
            rdraxd: RegisterCache::new(),
        }
    }
}

pub mod redundant_aux {
    use super::*;
    use crate::units::ElectricPotential;
    use uom::si::electric_potential::microvolt;
    use super::alias;

    /// Raw Redundant Aux register readings.
    pub struct Raw<const N: usize> {
        pub rdraxa: RegisterCacheData<N, RedundantAuxillaryA>,
        pub rdraxb: RegisterCacheData<N, RedundantAuxillaryB>,
        pub rdraxc: RegisterCacheData<N, RedundantAuxillaryC>,
        pub rdraxd: RegisterCacheData<N, RedundantAuxillaryD>,
    }
    
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
    pub struct NiceData<const N: usize> { inner: IndexByChip<N, NiceDataChip> }
    impl<const N: usize> core::ops::Deref for NiceData<N> {
        type Target = IndexByChip<N, NiceDataChip>;

        fn deref(&self) -> &Self::Target {
            &self.inner
        }
    }
    impl<const N: usize> TryFrom<Raw<N>> for NiceData<N> {
        type Error = ();

        /// Attempts to convert `Raw` data into a `NiceData`. If any of the registers involved
        /// in this `NiceData` haven't been read yet, this returns `Err(())`.
        fn try_from(raw: Raw<N>) -> Result<Self, Self::Error> {
            let Some(rdraxa) = raw.rdraxa.data() else { return Err(()); };
            let Some(rdraxb) = raw.rdraxb.data() else { return Err(()); };
            let Some(rdraxc) = raw.rdraxc.data() else { return Err(()); };
            let Some(rdraxd) = raw.rdraxd.data() else { return Err(()); };

            Ok(Self {
                inner: {
                    IndexByChip::from_fn(|chip| {
                        NiceDataChip {
                            gpio1_votlage: ElectricPotential::new::<microvolt>(rdraxa.get(chip).data().r_g1v().as_microvolts() as f32),
                            gpio2_votlage: ElectricPotential::new::<microvolt>(rdraxa.get(chip).data().r_g2v().as_microvolts() as f32),
                            gpio3_votlage: ElectricPotential::new::<microvolt>(rdraxa.get(chip).data().r_g3v().as_microvolts() as f32),

                            gpio4_votlage: ElectricPotential::new::<microvolt>(rdraxb.get(chip).data().r_g4v().as_microvolts() as f32),
                            gpio5_votlage: ElectricPotential::new::<microvolt>(rdraxb.get(chip).data().r_g5v().as_microvolts() as f32),
                            gpio6_votlage: ElectricPotential::new::<microvolt>(rdraxb.get(chip).data().r_g6v().as_microvolts() as f32),

                            gpio7_votlage: ElectricPotential::new::<microvolt>(rdraxc.get(chip).data().r_g7v().as_microvolts() as f32),
                            gpio8_votlage: ElectricPotential::new::<microvolt>(rdraxc.get(chip).data().r_g8v().as_microvolts() as f32),
                            gpio9_votlage: ElectricPotential::new::<microvolt>(rdraxc.get(chip).data().r_g9v().as_microvolts() as f32),

                            gpio10_votlage: ElectricPotential::new::<microvolt>(rdraxd.get(chip).data().r_g10v().as_microvolts() as f32),
                        }
                    })
                }
            })
        }
    }

    impl<const N: usize> CacheData<N> {
        /// Updates caches RedundantAuxillaryA through D with new data.
        /// 
        /// ### Returns
        /// Will return `Ok(())`, or `Err(UpdateError)` if an error occurred. If this returns `Ok(())`, the cached data was updated correctly and can be read now.
        pub async fn update_redundant_aux(&self, api: &mut alias::Api) -> Result<(), UpdateError> {
            use adbms6830b::chip::commands::adc::Aux2InputSelection;

            /// Autoconvert timeout in ms.
            const TIMEOUT_MS: u64 = 10_000;

            // u_TODO - double check this later. i think ADAX2 is what should be polled before reading but dunno. it might be ADAX2 plus normal ADAX?
            // or maybe no manual poll needs to be done at all if its a continuous conversion. but i forget
            // also make sure parameter is correct
            match api.adax2_autoconvert(Aux2InputSelection::All, TIMEOUT_MS).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_redundant_aux(): call to `service.adax2_autoconvert` resulted in an error. Error: {}", err);
                    return Err(UpdateError::PollError(err));
                }
            }

            self.rdraxa.update(api).await?;
            self.rdraxb.update(api).await?;
            self.rdraxc.update(api).await?;
            self.rdraxd.update(api).await?;

            Ok(())
        }

        /// Gets the current cached Redundant Aux data.
        pub fn get_redundant_aux(&self) -> redundant_aux::Raw<N> {
            redundant_aux::Raw {
                rdraxa: self.rdraxa.data(),
                rdraxb: self.rdraxb.data(),
                rdraxc: self.rdraxc.data(),
                rdraxd: self.rdraxd.data(),
            }
        }
    }
}