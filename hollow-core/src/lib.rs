// hollow-core/src/lib.rs
mod error;

pub use error::HollowError;

uniffi::setup_scaffolding!();
