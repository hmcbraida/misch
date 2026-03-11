use crate::error::MixError;
use crate::word::{MixWord, Sign};

#[derive(Debug, Clone, Copy)]
pub(crate) struct AddressSpec {
    pub(crate) address: i16,
    pub(crate) index: u8,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OperandSpec {
    pub(crate) addr: AddressSpec,
    pub(crate) field: u8,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum LoadTarget {
    A,
    X,
    I(u8),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum StoreSource {
    A,
    X,
    I(u8),
    J,
    Zero,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ShiftMode {
    Sla,
    Sra,
    Slax,
    Srax,
    Slc,
    Src,
    Slb,
    Srb,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum JumpCondition {
    Jmp,
    Jsj,
    Jov,
    Jnov,
    Jl,
    Je,
    Jg,
    Jge,
    Jne,
    Jle,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RegisterJumpTarget {
    A,
    X,
    I(u8),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RegisterJumpCondition {
    Negative,
    Zero,
    Positive,
    NonNegative,
    NonZero,
    NonPositive,
    Even,
    Odd,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AddrTransferTarget {
    A,
    X,
    I(u8),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AddrTransferMode {
    Inc,
    Dec,
    Ent,
    Enn,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CompareTarget {
    A,
    X,
    I(u8),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Instruction {
    Nop,
    Add(OperandSpec),
    Sub(OperandSpec),
    Mul(OperandSpec),
    Div(OperandSpec),
    Num,
    Char,
    Hlt,
    Shift {
        addr: AddressSpec,
        mode: ShiftMode,
    },
    Move {
        addr: AddressSpec,
        count: u8,
    },
    Load {
        target: LoadTarget,
        negate: bool,
        operand: OperandSpec,
    },
    Store {
        source: StoreSource,
        operand: OperandSpec,
    },
    Jbus {
        addr: AddressSpec,
        unit: u8,
    },
    Ioc {
        addr: AddressSpec,
        unit: u8,
    },
    In {
        addr: AddressSpec,
        unit: u8,
    },
    Out {
        addr: AddressSpec,
        unit: u8,
    },
    Jred {
        addr: AddressSpec,
        unit: u8,
    },
    Jump {
        addr: AddressSpec,
        cond: JumpCondition,
    },
    RegisterJump {
        addr: AddressSpec,
        target: RegisterJumpTarget,
        cond: RegisterJumpCondition,
    },
    AddrTransfer {
        addr: AddressSpec,
        target: AddrTransferTarget,
        mode: AddrTransferMode,
    },
    Compare {
        target: CompareTarget,
        operand: OperandSpec,
    },
}

pub(crate) fn decode(word: MixWord, byte_size: u16) -> Result<Instruction, MixError> {
    word.validate(byte_size)?;
    let addr_mag = i16::try_from(word.bytes[0] * byte_size + word.bytes[1]).unwrap();
    let address = if word.sign == Sign::Negative {
        -addr_mag
    } else {
        addr_mag
    };
    let index = u8::try_from(word.bytes[2]).unwrap();
    let field = u8::try_from(word.bytes[3]).unwrap();
    let opcode = u8::try_from(word.bytes[4]).unwrap();

    let addr = AddressSpec { address, index };
    let operand = OperandSpec { addr, field };

    match opcode {
        0 if field == 0 => Ok(Instruction::Nop),
        0 => Err(MixError::InvalidOpcode { opcode, field }),
        1 => Ok(Instruction::Add(operand)),
        2 => Ok(Instruction::Sub(operand)),
        3 => Ok(Instruction::Mul(operand)),
        4 => Ok(Instruction::Div(operand)),
        5 => match field {
            0 => Ok(Instruction::Num),
            1 => Ok(Instruction::Char),
            2 => Ok(Instruction::Hlt),
            _ => Err(MixError::InvalidOpcode { opcode, field }),
        },
        6 => {
            let mode = match field {
                0 => ShiftMode::Sla,
                1 => ShiftMode::Sra,
                2 => ShiftMode::Slax,
                3 => ShiftMode::Srax,
                4 => ShiftMode::Slc,
                5 => ShiftMode::Src,
                6 => ShiftMode::Slb,
                7 => ShiftMode::Srb,
                _ => return Err(MixError::InvalidOpcode { opcode, field }),
            };
            Ok(Instruction::Shift { addr, mode })
        }
        7 => Ok(Instruction::Move { addr, count: field }),
        8 => Ok(Instruction::Load {
            target: LoadTarget::A,
            negate: false,
            operand,
        }),
        9..=14 => Ok(Instruction::Load {
            target: LoadTarget::I(opcode - 8),
            negate: false,
            operand,
        }),
        15 => Ok(Instruction::Load {
            target: LoadTarget::X,
            negate: false,
            operand,
        }),
        16 => Ok(Instruction::Load {
            target: LoadTarget::A,
            negate: true,
            operand,
        }),
        17..=22 => Ok(Instruction::Load {
            target: LoadTarget::I(opcode - 16),
            negate: true,
            operand,
        }),
        23 => Ok(Instruction::Load {
            target: LoadTarget::X,
            negate: true,
            operand,
        }),
        24 => Ok(Instruction::Store {
            source: StoreSource::A,
            operand,
        }),
        25..=30 => Ok(Instruction::Store {
            source: StoreSource::I(opcode - 24),
            operand,
        }),
        31 => Ok(Instruction::Store {
            source: StoreSource::X,
            operand,
        }),
        32 => Ok(Instruction::Store {
            source: StoreSource::J,
            operand,
        }),
        33 => Ok(Instruction::Store {
            source: StoreSource::Zero,
            operand,
        }),
        34 => Ok(Instruction::Jbus { addr, unit: field }),
        35 => Ok(Instruction::Ioc { addr, unit: field }),
        36 => Ok(Instruction::In { addr, unit: field }),
        37 => Ok(Instruction::Out { addr, unit: field }),
        38 => Ok(Instruction::Jred { addr, unit: field }),
        39 => {
            let cond = match field {
                0 => JumpCondition::Jmp,
                1 => JumpCondition::Jsj,
                2 => JumpCondition::Jov,
                3 => JumpCondition::Jnov,
                4 => JumpCondition::Jl,
                5 => JumpCondition::Je,
                6 => JumpCondition::Jg,
                7 => JumpCondition::Jge,
                8 => JumpCondition::Jne,
                9 => JumpCondition::Jle,
                _ => return Err(MixError::InvalidOpcode { opcode, field }),
            };
            Ok(Instruction::Jump { addr, cond })
        }
        40 => Ok(Instruction::RegisterJump {
            addr,
            target: RegisterJumpTarget::A,
            cond: decode_register_jump_cond(field)?,
        }),
        41..=46 => {
            if field > 5 {
                return Err(MixError::InvalidOpcode { opcode, field });
            }
            Ok(Instruction::RegisterJump {
                addr,
                target: RegisterJumpTarget::I(opcode - 40),
                cond: decode_register_jump_cond(field)?,
            })
        }
        47 => Ok(Instruction::RegisterJump {
            addr,
            target: RegisterJumpTarget::X,
            cond: decode_register_jump_cond(field)?,
        }),
        48 => Ok(Instruction::AddrTransfer {
            addr,
            target: AddrTransferTarget::A,
            mode: decode_addr_transfer_mode(opcode, field)?,
        }),
        49..=54 => Ok(Instruction::AddrTransfer {
            addr,
            target: AddrTransferTarget::I(opcode - 48),
            mode: decode_addr_transfer_mode(opcode, field)?,
        }),
        55 => Ok(Instruction::AddrTransfer {
            addr,
            target: AddrTransferTarget::X,
            mode: decode_addr_transfer_mode(opcode, field)?,
        }),
        56 => Ok(Instruction::Compare {
            target: CompareTarget::A,
            operand,
        }),
        57..=62 => Ok(Instruction::Compare {
            target: CompareTarget::I(opcode - 56),
            operand,
        }),
        63 => Ok(Instruction::Compare {
            target: CompareTarget::X,
            operand,
        }),
        _ => Err(MixError::InvalidOpcode { opcode, field }),
    }
}

fn decode_register_jump_cond(field: u8) -> Result<RegisterJumpCondition, MixError> {
    match field {
        0 => Ok(RegisterJumpCondition::Negative),
        1 => Ok(RegisterJumpCondition::Zero),
        2 => Ok(RegisterJumpCondition::Positive),
        3 => Ok(RegisterJumpCondition::NonNegative),
        4 => Ok(RegisterJumpCondition::NonZero),
        5 => Ok(RegisterJumpCondition::NonPositive),
        6 => Ok(RegisterJumpCondition::Even),
        7 => Ok(RegisterJumpCondition::Odd),
        _ => Err(MixError::InvalidFieldSpec(field)),
    }
}

fn decode_addr_transfer_mode(opcode: u8, field: u8) -> Result<AddrTransferMode, MixError> {
    match field {
        0 => Ok(AddrTransferMode::Inc),
        1 => Ok(AddrTransferMode::Dec),
        2 => Ok(AddrTransferMode::Ent),
        3 => Ok(AddrTransferMode::Enn),
        _ => Err(MixError::InvalidOpcode { opcode, field }),
    }
}
