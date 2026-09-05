//! Module for caching SPI reads to the ADBMS6830B chips.

use adbms6830b::{chip::registers::{
    ReadableGroup,
    pwm::{PwmA, PwmB},
    results::{RedundantAuxillaryA, RedundantAuxillaryB, RedundantAuxillaryC, RedundantAuxillaryD},
}, turnkey::api::LineId};
use adbms6830b::line::Error;
use adbms6830b::turnkey::api::Responses;
use embedded_hal_async::i2c::NoAcknowledgeSource::Data;
use super::alias::{SpiError, Service};
use adbms6830b::line::PecStatus;
use super::chips::ChipId;
use core::cell::Cell;

/// Thin wrapper around an array of responses for each chip.
/// You can put any datatype in here for `T` as long as it makes
/// sense to index it by a ChipId.
/// 
/// The point of this so responses can be interacted with
/// via `ChipId` (and iterated over) instead of having to
/// lookup raw arrays (whuch might require you to convert a ChipId to usize).
#[derive(Copy, Clone, Debug)]
pub struct IndexByChip<const N: usize, T> {
    data: [T; N],
}
impl<const N: usize, T> IndexByChip<N, T> {
    /// Retrives the data for `chip`.
    pub const fn data(&self, chip: ChipId) -> &T {
        let i: usize = chip as usize;
        &self.data[i]
    }
    // u_TODO - probably implement iterator here possibly (i think iter is built in for arrays?)
}

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

    /// Register read data for a specific chip.
    /// If no read has been made yet, this is None.
    pub const fn data(&self, chip: super::chips::ChipId) -> Option<&Reading<R>> {
        match &self.data {
            Some(data) => Some(data.data(chip)),
            None => None,
        }
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
    pub async fn update(&self, service: &super::alias::Service) -> Result<(), UpdateError> {
        let data: [Reading<R>; N] = {
            let responses = service.read::<R>().await;

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
                data: Some(IndexByChip { data }),
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
    pub const fn new() -> Self {
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

    /// Cached Redundant Aux data.
    pub struct RedundantAux<const N: usize> {
        pub rdraxa: RegisterCacheData<N, RedundantAuxillaryA>,
        pub rdraxb: RegisterCacheData<N, RedundantAuxillaryB>,
        pub rdraxc: RegisterCacheData<N, RedundantAuxillaryC>,
        pub rdraxd: RegisterCacheData<N, RedundantAuxillaryD>,
    }

    impl<const N: usize> CacheData<N> {
        /// Updates caches RedundantAuxillaryA through D with new data.
        /// 
        /// ### Parameters
        /// - `service`: The `Service` belonging to the caller. This function is meant to be called by `Segments`, so this will likely be (`self.service`).
        /// 
        /// ### Returns
        /// Will return `Ok(())`, or `Err(UpdateError)` if an error occurred. If this returns `Ok(())`, the cached data was updated correctly and can be read now.
        pub async fn update_redundant_aux(&mut self, service: &Service) -> Result<(), UpdateError> {
            use adbms6830b::chip::commands::adc::Aux2InputSelection;

            /// Autoconvert timeout in ms.
            const TIMEOUT_MS: u64 = 10_000;

            // u_TODO - double check this later. i think ADAX2 is what should be polled before reading but dunno. it might be ADAX2 plus normal ADAX?
            // or maybe no manual poll needs to be done at all if its a continuous conversion. but i forget
            // also make sure parameter is correct
            match service.adax2_autoconvert(Aux2InputSelection::All, TIMEOUT_MS).await {
                Ok(_) => (),
                Err(err) => {
                    defmt::error!("Segments: Cache: in `update_redundant_aux(): call to `service.adax2_autoconvert` resulted in an error. Error: {}", err);
                    return Err(UpdateError::PollError(err));
                }
            }

            self.rdraxa.update(service).await?;
            self.rdraxb.update(service).await?;
            self.rdraxc.update(service).await?;
            self.rdraxd.update(service).await?;

            Ok(())
        }

        /// Gets the current cached Redundant Aux data.
        pub fn get_redundant_aux(&self) -> RedundantAux<N> {
            RedundantAux {
                rdraxa: self.rdraxa.data(),
                rdraxb: self.rdraxb.data(),
                rdraxc: self.rdraxc.data(),
                rdraxd: self.rdraxd.data(),
            }
        }
    }
}