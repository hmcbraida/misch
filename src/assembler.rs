use crate::MixError;
use crate::instruction::{
    AddrTransferMode, AddrTransferTarget, AddressSpec, CompareTarget,
    Instruction, JumpCondition, LoadTarget, OperandSpec, RegisterJumpCondition,
    RegisterJumpTarget, ShiftMode, StoreSource,
};
use crate::state::MixState;
use std::fmt;

const DEFAULT_BYTE_SIZE: u16 = 64;

struct ParsedOperand {
    address: i16,
    index: u8,
    field: u8,
    has_field: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblerError {
    Syntax { line: usize, message: String },
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

pub fn assemble(source: &str) -> Result<MixState, AssemblerError> {
    let mut state = MixState::blank(DEFAULT_BYTE_SIZE)?;
    let mut program_counter = 0usize;

    for (line_idx, raw_line) in source.lines().enumerate() {
        let line_no = line_idx + 1;
        let line = strip_comments(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        let mut parts = line.splitn(2, char::is_whitespace);
        let mnemonic = parts.next().unwrap().trim().to_ascii_uppercase();
        let operand = parts.next().unwrap_or("").trim();
        if mnemonic.is_empty() {
            continue;
        }

        if program_counter >= 4000 {
            return Err(asm_syntax(line_no, "program exceeds MIX memory size"));
        }

        let instruction = parse_instruction(&mnemonic, operand, line_no)?;
        state.set_memory_raw(
            program_counter,
            instruction.encode(DEFAULT_BYTE_SIZE),
        )?;
        program_counter += 1;
    }

    Ok(state)
}

fn parse_instruction(
    mnemonic: &str,
    operand_text: &str,
    line_no: usize,
) -> Result<Instruction, AssemblerError> {
    match mnemonic {
        "NOP" => {
            no_operand_instruction(operand_text, line_no, Instruction::Nop)
        }
        "NUM" => {
            no_operand_instruction(operand_text, line_no, Instruction::Num)
        }
        "CHAR" => {
            no_operand_instruction(operand_text, line_no, Instruction::Char)
        }
        "HLT" => {
            no_operand_instruction(operand_text, line_no, Instruction::Hlt)
        }

        "ADD" => {
            operand_instruction(operand_text, line_no, 5, Instruction::Add)
        }
        "SUB" => {
            operand_instruction(operand_text, line_no, 5, Instruction::Sub)
        }
        "MUL" => {
            operand_instruction(operand_text, line_no, 5, Instruction::Mul)
        }
        "DIV" => {
            operand_instruction(operand_text, line_no, 5, Instruction::Div)
        }
        "MOVE" => {
            let op = parse_operand(operand_text, line_no, 0)?;
            Ok(Instruction::Move {
                addr: address_from(&op),
                count: op.field,
            })
        }

        "SLA" => shift_instruction(operand_text, line_no, ShiftMode::Sla),
        "SRA" => shift_instruction(operand_text, line_no, ShiftMode::Sra),
        "SLAX" => shift_instruction(operand_text, line_no, ShiftMode::Slax),
        "SRAX" => shift_instruction(operand_text, line_no, ShiftMode::Srax),
        "SLC" => shift_instruction(operand_text, line_no, ShiftMode::Slc),
        "SRC" => shift_instruction(operand_text, line_no, ShiftMode::Src),
        "SLB" => shift_instruction(operand_text, line_no, ShiftMode::Slb),
        "SRB" => shift_instruction(operand_text, line_no, ShiftMode::Srb),

        "LDA" => load_instruction(operand_text, line_no, LoadTarget::A, false),
        "LD1" => {
            load_instruction(operand_text, line_no, LoadTarget::I(1), false)
        }
        "LD2" => {
            load_instruction(operand_text, line_no, LoadTarget::I(2), false)
        }
        "LD3" => {
            load_instruction(operand_text, line_no, LoadTarget::I(3), false)
        }
        "LD4" => {
            load_instruction(operand_text, line_no, LoadTarget::I(4), false)
        }
        "LD5" => {
            load_instruction(operand_text, line_no, LoadTarget::I(5), false)
        }
        "LD6" => {
            load_instruction(operand_text, line_no, LoadTarget::I(6), false)
        }
        "LDX" => load_instruction(operand_text, line_no, LoadTarget::X, false),
        "LDAN" => load_instruction(operand_text, line_no, LoadTarget::A, true),
        "LD1N" => {
            load_instruction(operand_text, line_no, LoadTarget::I(1), true)
        }
        "LD2N" => {
            load_instruction(operand_text, line_no, LoadTarget::I(2), true)
        }
        "LD3N" => {
            load_instruction(operand_text, line_no, LoadTarget::I(3), true)
        }
        "LD4N" => {
            load_instruction(operand_text, line_no, LoadTarget::I(4), true)
        }
        "LD5N" => {
            load_instruction(operand_text, line_no, LoadTarget::I(5), true)
        }
        "LD6N" => {
            load_instruction(operand_text, line_no, LoadTarget::I(6), true)
        }
        "LDXN" => load_instruction(operand_text, line_no, LoadTarget::X, true),

        "STA" => store_instruction(operand_text, line_no, StoreSource::A, 5),
        "ST1" => store_instruction(operand_text, line_no, StoreSource::I(1), 5),
        "ST2" => store_instruction(operand_text, line_no, StoreSource::I(2), 5),
        "ST3" => store_instruction(operand_text, line_no, StoreSource::I(3), 5),
        "ST4" => store_instruction(operand_text, line_no, StoreSource::I(4), 5),
        "ST5" => store_instruction(operand_text, line_no, StoreSource::I(5), 5),
        "ST6" => store_instruction(operand_text, line_no, StoreSource::I(6), 5),
        "STX" => store_instruction(operand_text, line_no, StoreSource::X, 5),
        "STJ" => store_instruction(operand_text, line_no, StoreSource::J, 2),
        "STZ" => store_instruction(operand_text, line_no, StoreSource::Zero, 5),

        "JBUS" => io_instruction(operand_text, line_no, IoKind::Jbus),
        "IOC" => io_instruction(operand_text, line_no, IoKind::Ioc),
        "IN" => io_instruction(operand_text, line_no, IoKind::In),
        "OUT" => io_instruction(operand_text, line_no, IoKind::Out),
        "JRED" => io_instruction(operand_text, line_no, IoKind::Jred),

        "JMP" => jump_instruction(operand_text, line_no, JumpCondition::Jmp),
        "JSJ" => jump_instruction(operand_text, line_no, JumpCondition::Jsj),
        "JOV" => jump_instruction(operand_text, line_no, JumpCondition::Jov),
        "JNOV" => jump_instruction(operand_text, line_no, JumpCondition::Jnov),
        "JL" => jump_instruction(operand_text, line_no, JumpCondition::Jl),
        "JE" => jump_instruction(operand_text, line_no, JumpCondition::Je),
        "JG" => jump_instruction(operand_text, line_no, JumpCondition::Jg),
        "JGE" => jump_instruction(operand_text, line_no, JumpCondition::Jge),
        "JNE" => jump_instruction(operand_text, line_no, JumpCondition::Jne),
        "JLE" => jump_instruction(operand_text, line_no, JumpCondition::Jle),

        "JAN" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::A,
            RegisterJumpCondition::Negative,
        ),
        "JAZ" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::A,
            RegisterJumpCondition::Zero,
        ),
        "JAP" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::A,
            RegisterJumpCondition::Positive,
        ),
        "JANN" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::A,
            RegisterJumpCondition::NonNegative,
        ),
        "JANZ" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::A,
            RegisterJumpCondition::NonZero,
        ),
        "JANP" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::A,
            RegisterJumpCondition::NonPositive,
        ),
        "JAE" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::A,
            RegisterJumpCondition::Even,
        ),
        "JAO" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::A,
            RegisterJumpCondition::Odd,
        ),

        "J1N" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::Negative,
        ),
        "J1Z" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::Zero,
        ),
        "J1P" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::Positive,
        ),
        "J1NN" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::NonNegative,
        ),
        "J1NZ" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::NonZero,
        ),
        "J1NP" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::NonPositive,
        ),

        "J2N" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::Negative,
        ),
        "J2Z" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::Zero,
        ),
        "J2P" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::Positive,
        ),
        "J2NN" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::NonNegative,
        ),
        "J2NZ" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::NonZero,
        ),
        "J2NP" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::NonPositive,
        ),

        "J3N" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::Negative,
        ),
        "J3Z" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::Zero,
        ),
        "J3P" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::Positive,
        ),
        "J3NN" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::NonNegative,
        ),
        "J3NZ" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::NonZero,
        ),
        "J3NP" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::NonPositive,
        ),

        "J4N" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::Negative,
        ),
        "J4Z" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::Zero,
        ),
        "J4P" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::Positive,
        ),
        "J4NN" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::NonNegative,
        ),
        "J4NZ" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::NonZero,
        ),
        "J4NP" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::NonPositive,
        ),

        "J5N" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::Negative,
        ),
        "J5Z" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::Zero,
        ),
        "J5P" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::Positive,
        ),
        "J5NN" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::NonNegative,
        ),
        "J5NZ" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::NonZero,
        ),
        "J5NP" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::NonPositive,
        ),

        "J6N" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::Negative,
        ),
        "J6Z" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::Zero,
        ),
        "J6P" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::Positive,
        ),
        "J6NN" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::NonNegative,
        ),
        "J6NZ" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::NonZero,
        ),
        "J6NP" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::NonPositive,
        ),

        "JXN" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::X,
            RegisterJumpCondition::Negative,
        ),
        "JXZ" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::X,
            RegisterJumpCondition::Zero,
        ),
        "JXP" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::X,
            RegisterJumpCondition::Positive,
        ),
        "JXNN" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::X,
            RegisterJumpCondition::NonNegative,
        ),
        "JXNZ" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::X,
            RegisterJumpCondition::NonZero,
        ),
        "JXNP" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::X,
            RegisterJumpCondition::NonPositive,
        ),
        "JXE" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::X,
            RegisterJumpCondition::Even,
        ),
        "JXO" => register_jump_instruction(
            operand_text,
            line_no,
            RegisterJumpTarget::X,
            RegisterJumpCondition::Odd,
        ),

        "INCA" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::A,
            AddrTransferMode::Inc,
        ),
        "DECA" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::A,
            AddrTransferMode::Dec,
        ),
        "ENTA" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::A,
            AddrTransferMode::Ent,
        ),
        "ENNA" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::A,
            AddrTransferMode::Enn,
        ),
        "INC1" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(1),
            AddrTransferMode::Inc,
        ),
        "DEC1" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(1),
            AddrTransferMode::Dec,
        ),
        "ENT1" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(1),
            AddrTransferMode::Ent,
        ),
        "ENN1" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(1),
            AddrTransferMode::Enn,
        ),
        "INC2" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(2),
            AddrTransferMode::Inc,
        ),
        "DEC2" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(2),
            AddrTransferMode::Dec,
        ),
        "ENT2" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(2),
            AddrTransferMode::Ent,
        ),
        "ENN2" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(2),
            AddrTransferMode::Enn,
        ),
        "INC3" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(3),
            AddrTransferMode::Inc,
        ),
        "DEC3" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(3),
            AddrTransferMode::Dec,
        ),
        "ENT3" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(3),
            AddrTransferMode::Ent,
        ),
        "ENN3" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(3),
            AddrTransferMode::Enn,
        ),
        "INC4" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(4),
            AddrTransferMode::Inc,
        ),
        "DEC4" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(4),
            AddrTransferMode::Dec,
        ),
        "ENT4" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(4),
            AddrTransferMode::Ent,
        ),
        "ENN4" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(4),
            AddrTransferMode::Enn,
        ),
        "INC5" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(5),
            AddrTransferMode::Inc,
        ),
        "DEC5" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(5),
            AddrTransferMode::Dec,
        ),
        "ENT5" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(5),
            AddrTransferMode::Ent,
        ),
        "ENN5" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(5),
            AddrTransferMode::Enn,
        ),
        "INC6" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(6),
            AddrTransferMode::Inc,
        ),
        "DEC6" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(6),
            AddrTransferMode::Dec,
        ),
        "ENT6" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(6),
            AddrTransferMode::Ent,
        ),
        "ENN6" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::I(6),
            AddrTransferMode::Enn,
        ),
        "INCX" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::X,
            AddrTransferMode::Inc,
        ),
        "DECX" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::X,
            AddrTransferMode::Dec,
        ),
        "ENTX" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::X,
            AddrTransferMode::Ent,
        ),
        "ENNX" => addr_transfer_instruction(
            operand_text,
            line_no,
            AddrTransferTarget::X,
            AddrTransferMode::Enn,
        ),

        "CMPA" => compare_instruction(operand_text, line_no, CompareTarget::A),
        "CMP1" => {
            compare_instruction(operand_text, line_no, CompareTarget::I(1))
        }
        "CMP2" => {
            compare_instruction(operand_text, line_no, CompareTarget::I(2))
        }
        "CMP3" => {
            compare_instruction(operand_text, line_no, CompareTarget::I(3))
        }
        "CMP4" => {
            compare_instruction(operand_text, line_no, CompareTarget::I(4))
        }
        "CMP5" => {
            compare_instruction(operand_text, line_no, CompareTarget::I(5))
        }
        "CMP6" => {
            compare_instruction(operand_text, line_no, CompareTarget::I(6))
        }
        "CMPX" => compare_instruction(operand_text, line_no, CompareTarget::X),

        _ => Err(asm_syntax(
            line_no,
            &format!("unknown mnemonic `{mnemonic}`"),
        )),
    }
}

fn no_operand_instruction(
    operand_text: &str,
    line_no: usize,
    instruction: Instruction,
) -> Result<Instruction, AssemblerError> {
    if !operand_text.is_empty() {
        return Err(asm_syntax(
            line_no,
            "instruction does not take an operand",
        ));
    }
    Ok(instruction)
}

fn operand_instruction<F>(
    operand_text: &str,
    line_no: usize,
    default_field: u8,
    builder: F,
) -> Result<Instruction, AssemblerError>
where
    F: FnOnce(OperandSpec) -> Instruction,
{
    let parsed = parse_operand(operand_text, line_no, default_field)?;
    Ok(builder(operand_from(&parsed)))
}

fn fixed_address_instruction<F>(
    operand_text: &str,
    line_no: usize,
    fixed_field: u8,
    builder: F,
) -> Result<Instruction, AssemblerError>
where
    F: FnOnce(AddressSpec) -> Instruction,
{
    let parsed = parse_operand(operand_text, line_no, fixed_field)?;
    if parsed.has_field {
        return Err(asm_syntax(
            line_no,
            "field cannot be overridden for this mnemonic",
        ));
    }
    Ok(builder(address_from(&parsed)))
}

fn shift_instruction(
    operand_text: &str,
    line_no: usize,
    mode: ShiftMode,
) -> Result<Instruction, AssemblerError> {
    fixed_address_instruction(operand_text, line_no, 0, |addr| {
        Instruction::Shift { addr, mode }
    })
}

fn load_instruction(
    operand_text: &str,
    line_no: usize,
    target: LoadTarget,
    negate: bool,
) -> Result<Instruction, AssemblerError> {
    operand_instruction(operand_text, line_no, 5, |operand| Instruction::Load {
        target,
        negate,
        operand,
    })
}

fn store_instruction(
    operand_text: &str,
    line_no: usize,
    source: StoreSource,
    default_field: u8,
) -> Result<Instruction, AssemblerError> {
    operand_instruction(operand_text, line_no, default_field, |operand| {
        Instruction::Store { source, operand }
    })
}

enum IoKind {
    Jbus,
    Ioc,
    In,
    Out,
    Jred,
}

fn io_instruction(
    operand_text: &str,
    line_no: usize,
    kind: IoKind,
) -> Result<Instruction, AssemblerError> {
    let parsed = parse_operand(operand_text, line_no, 0)?;
    let addr = address_from(&parsed);
    let unit = parsed.field;
    Ok(match kind {
        IoKind::Jbus => Instruction::Jbus { addr, unit },
        IoKind::Ioc => Instruction::Ioc { addr, unit },
        IoKind::In => Instruction::In { addr, unit },
        IoKind::Out => Instruction::Out { addr, unit },
        IoKind::Jred => Instruction::Jred { addr, unit },
    })
}

fn jump_instruction(
    operand_text: &str,
    line_no: usize,
    cond: JumpCondition,
) -> Result<Instruction, AssemblerError> {
    fixed_address_instruction(operand_text, line_no, 0, |addr| {
        Instruction::Jump { addr, cond }
    })
}

fn register_jump_instruction(
    operand_text: &str,
    line_no: usize,
    target: RegisterJumpTarget,
    cond: RegisterJumpCondition,
) -> Result<Instruction, AssemblerError> {
    fixed_address_instruction(operand_text, line_no, 0, |addr| {
        Instruction::RegisterJump { addr, target, cond }
    })
}

fn addr_transfer_instruction(
    operand_text: &str,
    line_no: usize,
    target: AddrTransferTarget,
    mode: AddrTransferMode,
) -> Result<Instruction, AssemblerError> {
    fixed_address_instruction(operand_text, line_no, 0, |addr| {
        Instruction::AddrTransfer { addr, target, mode }
    })
}

fn compare_instruction(
    operand_text: &str,
    line_no: usize,
    target: CompareTarget,
) -> Result<Instruction, AssemblerError> {
    operand_instruction(operand_text, line_no, 5, |operand| {
        Instruction::Compare { target, operand }
    })
}

fn address_from(op: &ParsedOperand) -> AddressSpec {
    AddressSpec {
        address: op.address,
        index: op.index,
    }
}

fn operand_from(op: &ParsedOperand) -> OperandSpec {
    OperandSpec {
        addr: address_from(op),
        field: op.field,
    }
}

fn parse_operand(
    operand_text: &str,
    line_no: usize,
    default_field: u8,
) -> Result<ParsedOperand, AssemblerError> {
    if operand_text.is_empty() {
        return Ok(ParsedOperand {
            address: 0,
            index: 0,
            field: default_field,
            has_field: false,
        });
    }

    let mut core = operand_text;
    let mut has_field = false;
    let mut field = default_field;

    if let Some(open_idx) = core.find('(') {
        if !core.ends_with(')') {
            return Err(asm_syntax(
                line_no,
                "field specification must end with `)`",
            ));
        }
        let close_idx = core.len() - 1;
        if open_idx >= close_idx {
            return Err(asm_syntax(line_no, "empty field specification"));
        }
        let field_text = core[open_idx + 1..close_idx].trim();
        field = parse_field(field_text, line_no)?;
        has_field = true;
        core = core[..open_idx].trim();
    }

    let (address_text, index_text) = if let Some(comma_idx) = core.find(',') {
        (core[..comma_idx].trim(), Some(core[comma_idx + 1..].trim()))
    } else {
        (core.trim(), None)
    };

    let address = if address_text.is_empty() {
        0
    } else {
        let parsed = address_text.parse::<i32>().map_err(|_| {
            asm_syntax(line_no, &format!("invalid address `{address_text}`"))
        })?;
        i16::try_from(parsed).map_err(|_| {
            asm_syntax(line_no, &format!("address `{parsed}` out of range"))
        })?
    };

    let index = match index_text {
        Some(text) if !text.is_empty() => {
            let parsed = text.parse::<u8>().map_err(|_| {
                asm_syntax(line_no, &format!("invalid index register `{text}`"))
            })?;
            if parsed > 6 {
                return Err(asm_syntax(
                    line_no,
                    &format!("index register `{parsed}` out of range"),
                ));
            }
            parsed
        }
        Some(_) => {
            return Err(asm_syntax(
                line_no,
                "missing index register after `,`",
            ));
        }
        None => 0,
    };

    Ok(ParsedOperand {
        address,
        index,
        field,
        has_field,
    })
}

fn parse_field(field_text: &str, line_no: usize) -> Result<u8, AssemblerError> {
    if field_text.is_empty() {
        return Err(asm_syntax(line_no, "empty field specification"));
    }

    if let Some(colon_idx) = field_text.find(':') {
        let left_text = field_text[..colon_idx].trim();
        let right_text = field_text[colon_idx + 1..].trim();
        let left = parse_field_component(left_text, line_no)?;
        let right = parse_field_component(right_text, line_no)?;
        if left > right {
            return Err(asm_syntax(
                line_no,
                "field specification must satisfy L <= R",
            ));
        }
        return Ok(left * 8 + right);
    }

    let packed = field_text.parse::<u8>().map_err(|_| {
        asm_syntax(
            line_no,
            &format!("invalid field specification `{field_text}`"),
        )
    })?;
    let left = packed / 8;
    let right = packed % 8;
    if left > right || right > 5 {
        return Err(asm_syntax(
            line_no,
            &format!("invalid field specification `{field_text}`"),
        ));
    }
    Ok(packed)
}

fn parse_field_component(
    value: &str,
    line_no: usize,
) -> Result<u8, AssemblerError> {
    let parsed = value.parse::<u8>().map_err(|_| {
        asm_syntax(line_no, &format!("invalid field component `{value}`"))
    })?;
    if parsed > 5 {
        return Err(asm_syntax(
            line_no,
            &format!("field component `{parsed}` out of range"),
        ));
    }
    Ok(parsed)
}

fn strip_comments(line: &str) -> &str {
    let hash_idx = line.find('#');
    let semi_idx = line.find(';');
    match (hash_idx, semi_idx) {
        (Some(h), Some(s)) => &line[..h.min(s)],
        (Some(h), None) => &line[..h],
        (None, Some(s)) => &line[..s],
        (None, None) => line,
    }
}

fn asm_syntax(line: usize, message: &str) -> AssemblerError {
    AssemblerError::Syntax {
        line,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_simple_program_and_runs() {
        let source = "LDA 2000\nADD 2001\nSTA 2002\nHLT\n";
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
        let source = "LDA 100,2(1:3)\nHLT\n";
        let machine = assemble(source).unwrap();
        assert!(machine.memory_word(0).unwrap() > 0);
    }

    #[test]
    fn rejects_unknown_mnemonics() {
        let result = assemble("NOTREAL 0\n");
        assert!(matches!(
            result,
            Err(AssemblerError::Syntax { line: 1, .. })
        ));
    }

    #[test]
    fn rejects_fixed_field_override() {
        let result = assemble("JMP 10(0:1)\n");
        assert!(matches!(
            result,
            Err(AssemblerError::Syntax { line: 1, .. })
        ));
    }
}
