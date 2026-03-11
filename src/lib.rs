//! MIX machine core emulator.

mod assembler;
mod error;
mod instruction;
mod io;
mod state;
mod word;

pub use assembler::assemble;
pub use error::MixError;
pub use state::MixState;
