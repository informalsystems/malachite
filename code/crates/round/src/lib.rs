//! Per-round consensus state machine

#![forbid(unsafe_code)]
#![deny(unused_crate_dependencies, trivial_casts, trivial_numeric_casts)]
#![warn(
    missing_docs,
    rustdoc::broken_intra_doc_links,
    rustdoc::private_intra_doc_links,
    variant_size_differences
)]
// no_std compatibility
#![cfg_attr(not(feature = "std"), no_std)]
// For coverage on nightly
#![allow(unexpected_cfgs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::panic))]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

extern crate alloc;

pub mod input;
pub mod output;
pub mod state;
pub mod state_machine;
pub mod transition;

mod traces;
