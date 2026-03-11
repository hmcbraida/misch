//! MIX machine core emulator.

mod error;
mod instruction;
mod io;
mod state;
mod word;

pub use error::MixError;
pub use state::MixState;
