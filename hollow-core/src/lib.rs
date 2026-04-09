// hollow-core/src/lib.rs
mod db;
mod error;

pub use error::HollowError;

uniffi::setup_scaffolding!();
