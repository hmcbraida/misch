use std::fmt;

/// Errors produced while decoding or executing MIX instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixError {
    InvalidByteSize(u16),
    ByteOutOfRange {
        value: u16,
        byte_size: u16,
    },
    AddressOutOfRange(i32),
    InvalidIndexRegister(u8),
    InvalidFieldSpec(u8),
    InvalidOpcode {
        opcode: u8,
        field: u8,
    },
    DeviceUnitOutOfRange(u8),
    DeviceNotAttached(u8),
    WrongDeviceDirection {
        unit: u8,
        expected: &'static str,
    },
    DeviceBlockSizeMismatch {
        unit: u8,
        expected: usize,
        got: usize,
    },
    DivisionByZero,
    DivisionOverflow,
}

impl fmt::Display for MixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for MixError {}
