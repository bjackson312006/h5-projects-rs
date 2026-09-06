//! Module for controlling the Segments and their ADBMS6830B chips.
//! 
//! For context, there are 5 segments, each with two ADBMS6830B chips. So, there are 10 ADBMS6830B chips total.

use static_cell::StaticCell;
use embassy_time::{ Timer };

mod cache;
mod chips;
mod core;

pub use core::task::segments_task;