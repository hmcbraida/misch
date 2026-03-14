//! MIXAL assembler implementation.
//!
//! High-level procedure:
//! - Parse source text into a compact line model.
//! - Run a first pass to build symbol tables and assign locations.
//! - Run a second pass to encode instructions/directives into [`MixState`] memory.

use crate::MixError;
use crate::state::MixState;
use std::fmt;

mod parse;
mod pass1;
mod pass2;

use parse::parse_source;
pub(crate) use pass1::SymbolTables;
use pass1::first_pass;
use pass2::second_pass;

pub(crate) const DEFAULT_BYTE_SIZE: u16 = 64;
pub(crate) const MIX_MEMORY_SIZE: i64 = 4000;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Errors returned while assembling MIXAL source text.
pub enum AssemblerError {
    /// Syntax or semantic issue tied to a specific source line.
    Syntax { line: usize, message: String },
    /// Error propagated from machine-level validation or encoding.
    Machine(MixError),
}

impl fmt::Display for AssemblerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AssemblerError {}

impl From<MixError> for AssemblerError {
    fn from(value: MixError) -> Self {
        Self::Machine(value)
    }
}

#[derive(Debug, Clone, Copy)]
/// Expression evaluation context to improve diagnostics and symbol rules.
pub(crate) enum OperandComponent {
    Address,
    Index,
    Field,
    WExpr,
}

#[derive(Debug, Clone)]
/// Shared context used when evaluating expressions and operands.
pub(crate) struct EvalContext<'a> {
    pub(crate) symbols: &'a SymbolTables,
    pub(crate) line_no: usize,
    pub(crate) order: usize,
    pub(crate) location: i64,
    pub(crate) allow_future_standalone: bool,
    pub(crate) expression_text: &'a str,
}

pub(crate) fn ensure_location_in_memory(
    location: i64,
    line_no: usize,
) -> Result<(), AssemblerError> {
    if !(0..MIX_MEMORY_SIZE).contains(&location) {
        return Err(asm_syntax(
            line_no,
            &format!("address `{location}` out of MIX memory range"),
        ));
    }
    Ok(())
}

pub(crate) fn asm_syntax(line: usize, message: &str) -> AssemblerError {
    AssemblerError::Syntax {
        line,
        message: message.to_owned(),
    }
}

/// Assembles MIXAL source code into an initialized [`MixState`].
pub fn assemble(source: &str) -> Result<MixState, AssemblerError> {
    let parsed = parse_source(source)?;
    let first_pass = first_pass(&parsed)?;
    second_pass(&first_pass)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_simple_program_and_runs() {
        let source = "LDA 2000\nADD 2001\nSTA 2002\nHLT\nEND 0\n";
        let mut machine = assemble(source).unwrap();
        machine.set_memory_word(2000, 2).unwrap();
        machine.set_memory_word(2001, 3).unwrap();

        while !machine.is_halted() {
            machine.advance_state().unwrap();
        }

        assert_eq!(machine.memory_word(2002).unwrap(), 5);
    }

    #[test]
    fn supports_index_and_field_operand_parts() {
        let source = "LDA 100,2(1:3)\nHLT\nEND 0\n";
        let machine = assemble(source).unwrap();
        assert!(machine.memory_word(0).unwrap() > 0);
    }

    #[test]
    fn rejects_unknown_mnemonics() {
        let result = assemble("NOTREAL 0\nEND 0\n");
        assert!(matches!(
            result,
            Err(AssemblerError::Syntax { line: 1, .. })
        ));
    }

    #[test]
    fn rejects_fixed_field_override() {
        let result = assemble("JMP 10(0:1)\nEND 0\n");
        assert!(matches!(
            result,
            Err(AssemblerError::Syntax { line: 1, .. })
        ));
    }

    #[test]
    fn assembles_io_unit_numbers() {
        let source = "IN 2000(16)\nOUT 2000(18)\nHLT\nEND 0\n";
        let machine = assemble(source).unwrap();
        assert!(machine.memory_word(0).unwrap() > 0);
        assert!(machine.memory_word(1).unwrap() > 0);
    }

    #[test]
    fn supports_directives_and_labels() {
        let source = "START EQU 2000\nORIG START\nL1 LDA VALUE\nHLT\nVALUE CON 7\nEND L1\n";
        let mut machine = assemble(source).unwrap();
        while !machine.is_halted() {
            machine.advance_state().unwrap();
        }
        assert_eq!(machine.register_a(), 7);
    }

    #[test]
    fn supports_alf_and_literal_constants() {
        let source = "ORIG 3000\nMSG ALF \"HELLO\"\nLDA =15=\nHLT\nEND 3001\n";
        let mut machine = assemble(source).unwrap();
        machine.advance_state().unwrap();
        assert_eq!(machine.register_a(), 15);
        assert!(machine.memory_word(3000).unwrap() > 0);
    }

    #[test]
    fn supports_local_symbols() {
        let source = "ORIG 2000\n1H NOP\nJMP 1B\nJMP 3F\n3H HLT\nEND 2001\n";
        let machine = assemble(source).unwrap();
        assert!(machine.memory_word(2001).unwrap() > 0);
    }

    #[test]
    fn rejects_h_flavor_local_symbol_reference() {
        let source = "ORIG 2000\n1H NOP\nJMP 1H\nEND 2000\n";
        let result = assemble(source);
        assert!(matches!(
            result,
            Err(AssemblerError::Syntax { line: 3, message })
                if message.contains("undefined symbol `1H`")
        ));
    }

    #[test]
    fn rejects_missing_end() {
        let result = assemble("HLT\n");
        assert!(matches!(
            result,
            Err(AssemblerError::Syntax { message, .. }) if message.contains("END")
        ));
    }

    #[test]
    fn rejects_overflowing_expressions_without_panicking() {
        let result = assemble("A EQU 9223372036854775807+1\nHLT\nEND 0\n");
        assert!(matches!(
            result,
            Err(AssemblerError::Syntax { line: 1, message })
                if message.contains("overflow")
        ));
    }

    #[test]
    fn assembles_complicated_feature_rich_program() {
        let source = r#"
* complete MIXAL feature exercise
A EQU 14/3
B EQU 1:2
C EQU 1//64
START EQU 1200
ORIG START
ENTA 0 ; initialize accumulator
1H INCA 1
CMPA =3=
JL 1B
STA VALUE # save loop result
JMP 2F
ORIG *+1
VALUE CON -A(0:0),B(1:2),C(3:5)
TEXT ALF "HELLO"
2H LDA VALUE,1-1(0:5)
ADD =1=
STA VALUE
LDA FUTURE
JMP DONE
FUTURE CON 10
DONE HLT
END START
"#;

        let mut machine = assemble(source).unwrap();
        while !machine.is_halted() {
            machine.advance_state().unwrap();
        }

        assert_eq!(machine.register_a(), 10);
        assert_eq!(machine.memory_word(1207).unwrap(), 4);
        assert_eq!(machine.memory_word(1216).unwrap(), 3);
        assert_eq!(machine.memory_word(1217).unwrap(), 1);
    }
}
