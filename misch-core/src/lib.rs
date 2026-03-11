//! MIX machine core emulator.

mod assembler;
mod error;
mod instruction;
mod io;
mod mixchar;
mod state;
mod word;

pub use assembler::AssemblerError;
pub use assembler::assemble;
pub use error::MixError;
pub use mixchar::MixCharError;
pub use mixchar::decode_word_to_text;
pub use mixchar::decode_words_to_text;
pub use mixchar::encode_text_to_words;
pub use state::MixState;
