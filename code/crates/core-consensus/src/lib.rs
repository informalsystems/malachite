#![doc = include_str!("../README.md")]
#![allow(rustdoc::private_intra_doc_links)]

mod prelude;

mod input;
pub use input::Input;

mod state;
pub use state::State;

mod error;
pub use error::Error;

mod params;
pub use params::{Params, ThresholdParams};

// FaB: Removed HIDDEN_LOCK_ROUND export - not used in FaB

mod effect;
pub use effect::{Effect, Resumable, Resume};

mod types;
pub use types::*;

pub mod full_proposal;
pub mod util;

mod macros;
mod ser;

// Only used in macros
#[doc(hidden)]
pub mod gen;

// Only used in macros
mod handle;
#[doc(hidden)]
pub use handle::handle;

// Used in macros
#[doc(hidden)]
pub use tracing;
