use std::fmt;

/// Errors produced while decoding or executing MIX instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixError {
    /// Attempted to create a machine with an unsupported byte size.
    InvalidByteSize(u16),
    /// Encountered a byte value that is not valid for the configured byte size.
    ByteOutOfRange {
        /// Offending byte value.
        value: u16,
        /// Inclusive upper bound is `byte_size - 1`.
        byte_size: u16,
    },
    /// Referenced a memory address outside `0..4000`.
    AddressOutOfRange(i32),
    /// Used an index register outside the MIX range (`0..=6` or `1..=6`).
    InvalidIndexRegister(u8),
    /// Used an invalid packed field specification (`8 * L + R`).
    InvalidFieldSpec(u8),
    /// Used an opcode/field combination that is not defined by this emulator.
    InvalidOpcode {
        /// Opcode (`C`) byte.
        opcode: u8,
        /// Field (`F`) byte.
        field: u8,
    },
    /// Referenced a device unit outside `0..=20`.
    DeviceUnitOutOfRange(u8),
    /// Tried to use a device unit that has no callback attached.
    DeviceNotAttached(u8),
    /// Tried to execute input/output against a device in the wrong direction.
    WrongDeviceDirection {
        /// Device unit referenced by the instruction.
        unit: u8,
        /// Expected direction (`"input"` or `"output"`).
        expected: &'static str,
    },
    /// A callback returned a block with the wrong number of words.
    DeviceBlockSizeMismatch {
        /// Device unit that produced the block.
        unit: u8,
        /// Configured block size.
        expected: usize,
        /// Returned block size.
        got: usize,
    },
    /// Division attempted with a zero divisor.
    DivisionByZero,
    /// Division quotient does not fit in register `A` per MIX rules.
    DivisionOverflow,
}

impl fmt::Display for MixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for MixError {}
