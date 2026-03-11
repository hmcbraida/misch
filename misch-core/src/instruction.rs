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

impl Instruction {
    pub(crate) fn decode(
        word: MixWord,
        byte_size: u16,
    ) -> Result<Instruction, MixError> {
        word.validate(byte_size)?;
        let addr_mag =
            i16::try_from(word.bytes[0] * byte_size + word.bytes[1]).unwrap();
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

    pub(crate) fn encode(&self, byte_size: u16) -> MixWord {
        let (address, index, field, opcode) = match *self {
            Instruction::Nop => (0, 0, 0, 0),
            Instruction::Add(op) => {
                (op.addr.address, op.addr.index, op.field, 1)
            }
            Instruction::Sub(op) => {
                (op.addr.address, op.addr.index, op.field, 2)
            }
            Instruction::Mul(op) => {
                (op.addr.address, op.addr.index, op.field, 3)
            }
            Instruction::Div(op) => {
                (op.addr.address, op.addr.index, op.field, 4)
            }
            Instruction::Num => (0, 0, 0, 5),
            Instruction::Char => (0, 0, 1, 5),
            Instruction::Hlt => (0, 0, 2, 5),
            Instruction::Shift { addr, mode } => {
                let field = match mode {
                    ShiftMode::Sla => 0,
                    ShiftMode::Sra => 1,
                    ShiftMode::Slax => 2,
                    ShiftMode::Srax => 3,
                    ShiftMode::Slc => 4,
                    ShiftMode::Src => 5,
                    ShiftMode::Slb => 6,
                    ShiftMode::Srb => 7,
                };
                (addr.address, addr.index, field, 6)
            }
            Instruction::Move { addr, count } => {
                (addr.address, addr.index, count, 7)
            }
            Instruction::Load {
                target,
                negate,
                operand: op,
            } => {
                let opcode = match target {
                    LoadTarget::A if !negate => 8,
                    LoadTarget::I(i) if !negate => 8 + i,
                    LoadTarget::X if !negate => 15,
                    LoadTarget::A => 16,
                    LoadTarget::I(i) => 16 + i,
                    LoadTarget::X => 23,
                };
                (op.addr.address, op.addr.index, op.field, opcode)
            }
            Instruction::Store {
                source,
                operand: op,
            } => {
                let opcode = match source {
                    StoreSource::A => 24,
                    StoreSource::I(i) => 24 + i,
                    StoreSource::X => 31,
                    StoreSource::J => 32,
                    StoreSource::Zero => 33,
                };
                (op.addr.address, op.addr.index, op.field, opcode)
            }
            Instruction::Jbus { addr, unit } => {
                (addr.address, addr.index, unit, 34)
            }
            Instruction::Ioc { addr, unit } => {
                (addr.address, addr.index, unit, 35)
            }
            Instruction::In { addr, unit } => {
                (addr.address, addr.index, unit, 36)
            }
            Instruction::Out { addr, unit } => {
                (addr.address, addr.index, unit, 37)
            }
            Instruction::Jred { addr, unit } => {
                (addr.address, addr.index, unit, 38)
            }
            Instruction::Jump { addr, cond } => {
                let field = match cond {
                    JumpCondition::Jmp => 0,
                    JumpCondition::Jsj => 1,
                    JumpCondition::Jov => 2,
                    JumpCondition::Jnov => 3,
                    JumpCondition::Jl => 4,
                    JumpCondition::Je => 5,
                    JumpCondition::Jg => 6,
                    JumpCondition::Jge => 7,
                    JumpCondition::Jne => 8,
                    JumpCondition::Jle => 9,
                };
                (addr.address, addr.index, field, 39)
            }
            Instruction::RegisterJump { addr, target, cond } => {
                let opcode = match target {
                    RegisterJumpTarget::A => 40,
                    RegisterJumpTarget::I(i) => 40 + i,
                    RegisterJumpTarget::X => 47,
                };
                let field = match cond {
                    RegisterJumpCondition::Negative => 0,
                    RegisterJumpCondition::Zero => 1,
                    RegisterJumpCondition::Positive => 2,
                    RegisterJumpCondition::NonNegative => 3,
                    RegisterJumpCondition::NonZero => 4,
                    RegisterJumpCondition::NonPositive => 5,
                    RegisterJumpCondition::Even => 6,
                    RegisterJumpCondition::Odd => 7,
                };
                (addr.address, addr.index, field, opcode)
            }
            Instruction::AddrTransfer { addr, target, mode } => {
                let opcode = match target {
                    AddrTransferTarget::A => 48,
                    AddrTransferTarget::I(i) => 48 + i,
                    AddrTransferTarget::X => 55,
                };
                let field = match mode {
                    AddrTransferMode::Inc => 0,
                    AddrTransferMode::Dec => 1,
                    AddrTransferMode::Ent => 2,
                    AddrTransferMode::Enn => 3,
                };
                (addr.address, addr.index, field, opcode)
            }
            Instruction::Compare {
                target,
                operand: op,
            } => {
                let opcode = match target {
                    CompareTarget::A => 56,
                    CompareTarget::I(i) => 56 + i,
                    CompareTarget::X => 63,
                };
                (op.addr.address, op.addr.index, op.field, opcode)
            }
        };

        let sign = if address < 0 {
            Sign::Negative
        } else {
            Sign::Positive
        };
        let abs = address.unsigned_abs();

        MixWord {
            sign,
            bytes: [
                abs / byte_size,
                abs % byte_size,
                index as u16,
                field as u16,
                opcode as u16,
            ],
        }
    }
}

fn decode_register_jump_cond(
    field: u8,
) -> Result<RegisterJumpCondition, MixError> {
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

fn decode_addr_transfer_mode(
    opcode: u8,
    field: u8,
) -> Result<AddrTransferMode, MixError> {
    match field {
        0 => Ok(AddrTransferMode::Inc),
        1 => Ok(AddrTransferMode::Dec),
        2 => Ok(AddrTransferMode::Ent),
        3 => Ok(AddrTransferMode::Enn),
        _ => Err(MixError::InvalidOpcode { opcode, field }),
    }
}
