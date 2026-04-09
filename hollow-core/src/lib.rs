// hollow-core/src/lib.rs
mod db;
mod error;
mod store;

pub use error::HollowError;

uniffi::setup_scaffolding!();
