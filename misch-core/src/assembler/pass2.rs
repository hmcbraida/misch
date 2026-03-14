use super::pass1::{FirstPass, ItemKind};
use super::{
    AssemblerError, DEFAULT_BYTE_SIZE, EvalContext, OperandComponent,
    asm_syntax, ensure_location_in_memory,
};
use crate::MixCharError;
use crate::instruction::{
    AddrTransferMode, AddrTransferTarget, AddressSpec, CompareTarget,
    Instruction, JumpCondition, LoadTarget, OperandSpec, RegisterJumpCondition,
    RegisterJumpTarget, ShiftMode, StoreSource,
};
use crate::mixchar::encode_text_to_words;
use crate::state::MixState;
use crate::word::MixWord;

#[derive(Debug, Clone)]
/// Deferred literal allocated during operand parsing (e.g. `=5=`).
struct LiteralEntry {
    addr: i64,
    wexpr: String,
    line_no: usize,
    order: usize,
}

/// Second assembly pass.
///
/// Encodes all first-pass items into memory and emits deferred literal words.
pub(crate) fn second_pass(
    first: &FirstPass,
) -> Result<MixState, AssemblerError> {
    // Pass 2 encodes all queued items and then emits deferred literals.
    let mut state = MixState::blank(DEFAULT_BYTE_SIZE)?;
    let mut literal_entries: Vec<LiteralEntry> = Vec::new();
    let mut next_literal_addr = first.literal_start;

    for item in &first.items {
        ensure_location_in_memory(item.location, item.line_no)?;
        let ctx = EvalContext {
            symbols: &first.symbols,
            line_no: item.line_no,
            order: item.order,
            location: item.location,
            allow_future_standalone: false,
            expression_text: "",
        };

        let encoded = match &item.kind {
            ItemKind::Instruction { mnemonic, operand } => {
                let mut op_ctx = ctx.clone();
                op_ctx.expression_text = operand;
                let instruction = build_instruction(
                    mnemonic,
                    operand,
                    &mut op_ctx,
                    &mut literal_entries,
                    &mut next_literal_addr,
                )?;
                instruction.encode(DEFAULT_BYTE_SIZE)
            }
            ItemKind::Con { operand } => {
                let value = eval_w_expression(
                    operand,
                    &EvalContext {
                        expression_text: operand,
                        ..ctx.clone()
                    },
                    OperandComponent::WExpr,
                )?;
                MixWord::from_signed(value, DEFAULT_BYTE_SIZE)
            }
            ItemKind::Alf { operand } => {
                assemble_alf_word(operand, item.line_no)?
            }
        };

        state.set_memory_raw(item.location as usize, encoded)?;
    }

    for lit in &literal_entries {
        ensure_location_in_memory(lit.addr, lit.line_no)?;
        let value = eval_w_expression(
            &lit.wexpr,
            &EvalContext {
                symbols: &first.symbols,
                line_no: lit.line_no,
                order: lit.order,
                location: lit.addr,
                allow_future_standalone: false,
                expression_text: &lit.wexpr,
            },
            OperandComponent::WExpr,
        )?;
        state.set_memory_raw(
            lit.addr as usize,
            MixWord::from_signed(value, DEFAULT_BYTE_SIZE),
        )?;
    }

    state.set_instruction_counter(first.end_start as u16)?;

    Ok(state)
}

/// Parses and validates an `ALF` payload into exactly 5 MIX characters.
fn assemble_alf_word(
    operand: &str,
    line_no: usize,
) -> Result<MixWord, AssemblerError> {
    let text = parse_alf_text(operand, line_no)?;
    let words = encode_text_to_words(&text).map_err(|err| match err {
        MixCharError::UnsupportedCharacter(ch) => {
            asm_syntax(line_no, &format!("unsupported ALF character `{ch}`"))
        }
    })?;
    let value = words.first().copied().unwrap_or(0);
    Ok(MixWord::from_signed(value, DEFAULT_BYTE_SIZE))
}

/// Normalizes `ALF` source operand (quoted or bare) and pads to 5 chars.
fn parse_alf_text(
    operand: &str,
    line_no: usize,
) -> Result<String, AssemblerError> {
    let trimmed = operand.trim();
    if trimmed.is_empty() {
        return Err(asm_syntax(line_no, "`ALF` requires an operand"));
    }

    let mut text = if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        if trimmed.len() < 2 {
            return Err(asm_syntax(line_no, "invalid quoted `ALF` operand"));
        }
        trimmed[1..trimmed.len() - 1].to_owned()
    } else {
        trimmed.to_owned()
    };

    if text.chars().count() > 5 {
        return Err(asm_syntax(
            line_no,
            "`ALF` operand must contain at most 5 characters",
        ));
    }
    while text.chars().count() < 5 {
        text.push(' ');
    }
    Ok(text)
}

/// Converts one mnemonic + operand text into a typed instruction variant.
fn build_instruction(
    mnemonic: &str,
    operand_text: &str,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<Instruction, AssemblerError> {
    // Table-style mnemonic dispatch into typed instruction constructors.
    match mnemonic {
        "NOP" => {
            no_operand_instruction(operand_text, ctx.line_no, Instruction::Nop)
        }
        "NUM" => {
            no_operand_instruction(operand_text, ctx.line_no, Instruction::Num)
        }
        "CHAR" => {
            no_operand_instruction(operand_text, ctx.line_no, Instruction::Char)
        }
        "HLT" => {
            no_operand_instruction(operand_text, ctx.line_no, Instruction::Hlt)
        }
        "ADD" => operand_instruction(
            operand_text,
            5,
            ctx,
            literal_entries,
            next_literal_addr,
            Instruction::Add,
        ),
        "SUB" => operand_instruction(
            operand_text,
            5,
            ctx,
            literal_entries,
            next_literal_addr,
            Instruction::Sub,
        ),
        "MUL" => operand_instruction(
            operand_text,
            5,
            ctx,
            literal_entries,
            next_literal_addr,
            Instruction::Mul,
        ),
        "DIV" => operand_instruction(
            operand_text,
            5,
            ctx,
            literal_entries,
            next_literal_addr,
            Instruction::Div,
        ),
        "MOVE" => {
            let op = parse_operand_value(
                operand_text,
                0,
                false,
                true,
                ctx,
                literal_entries,
                next_literal_addr,
            )?;
            Ok(Instruction::Move {
                addr: AddressSpec {
                    address: op.address,
                    index: op.index,
                },
                count: op.field,
            })
        }
        "SLA" => shift_instruction(
            operand_text,
            ShiftMode::Sla,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "SRA" => shift_instruction(
            operand_text,
            ShiftMode::Sra,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "SLAX" => shift_instruction(
            operand_text,
            ShiftMode::Slax,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "SRAX" => shift_instruction(
            operand_text,
            ShiftMode::Srax,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "SLC" => shift_instruction(
            operand_text,
            ShiftMode::Slc,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "SRC" => shift_instruction(
            operand_text,
            ShiftMode::Src,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "SLB" => shift_instruction(
            operand_text,
            ShiftMode::Slb,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "SRB" => shift_instruction(
            operand_text,
            ShiftMode::Srb,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LDA" => load_instruction(
            operand_text,
            LoadTarget::A,
            false,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD1" => load_instruction(
            operand_text,
            LoadTarget::I(1),
            false,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD2" => load_instruction(
            operand_text,
            LoadTarget::I(2),
            false,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD3" => load_instruction(
            operand_text,
            LoadTarget::I(3),
            false,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD4" => load_instruction(
            operand_text,
            LoadTarget::I(4),
            false,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD5" => load_instruction(
            operand_text,
            LoadTarget::I(5),
            false,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD6" => load_instruction(
            operand_text,
            LoadTarget::I(6),
            false,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LDX" => load_instruction(
            operand_text,
            LoadTarget::X,
            false,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LDAN" => load_instruction(
            operand_text,
            LoadTarget::A,
            true,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD1N" => load_instruction(
            operand_text,
            LoadTarget::I(1),
            true,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD2N" => load_instruction(
            operand_text,
            LoadTarget::I(2),
            true,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD3N" => load_instruction(
            operand_text,
            LoadTarget::I(3),
            true,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD4N" => load_instruction(
            operand_text,
            LoadTarget::I(4),
            true,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD5N" => load_instruction(
            operand_text,
            LoadTarget::I(5),
            true,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LD6N" => load_instruction(
            operand_text,
            LoadTarget::I(6),
            true,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "LDXN" => load_instruction(
            operand_text,
            LoadTarget::X,
            true,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "STA" => store_instruction(
            operand_text,
            StoreSource::A,
            5,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ST1" => store_instruction(
            operand_text,
            StoreSource::I(1),
            5,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ST2" => store_instruction(
            operand_text,
            StoreSource::I(2),
            5,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ST3" => store_instruction(
            operand_text,
            StoreSource::I(3),
            5,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ST4" => store_instruction(
            operand_text,
            StoreSource::I(4),
            5,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ST5" => store_instruction(
            operand_text,
            StoreSource::I(5),
            5,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ST6" => store_instruction(
            operand_text,
            StoreSource::I(6),
            5,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "STX" => store_instruction(
            operand_text,
            StoreSource::X,
            5,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "STJ" => store_instruction(
            operand_text,
            StoreSource::J,
            2,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "STZ" => store_instruction(
            operand_text,
            StoreSource::Zero,
            5,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JBUS" => io_instruction(
            operand_text,
            IoKind::Jbus,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "IOC" => io_instruction(
            operand_text,
            IoKind::Ioc,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "IN" => io_instruction(
            operand_text,
            IoKind::In,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "OUT" => io_instruction(
            operand_text,
            IoKind::Out,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JRED" => io_instruction(
            operand_text,
            IoKind::Jred,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JMP" => jump_instruction(
            operand_text,
            JumpCondition::Jmp,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JSJ" => jump_instruction(
            operand_text,
            JumpCondition::Jsj,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JOV" => jump_instruction(
            operand_text,
            JumpCondition::Jov,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JNOV" => jump_instruction(
            operand_text,
            JumpCondition::Jnov,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JL" => jump_instruction(
            operand_text,
            JumpCondition::Jl,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JE" => jump_instruction(
            operand_text,
            JumpCondition::Je,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JG" => jump_instruction(
            operand_text,
            JumpCondition::Jg,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JGE" => jump_instruction(
            operand_text,
            JumpCondition::Jge,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JNE" => jump_instruction(
            operand_text,
            JumpCondition::Jne,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JLE" => jump_instruction(
            operand_text,
            JumpCondition::Jle,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JAN" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::A,
            RegisterJumpCondition::Negative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JAZ" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::A,
            RegisterJumpCondition::Zero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JAP" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::A,
            RegisterJumpCondition::Positive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JANN" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::A,
            RegisterJumpCondition::NonNegative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JANZ" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::A,
            RegisterJumpCondition::NonZero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JANP" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::A,
            RegisterJumpCondition::NonPositive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JAE" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::A,
            RegisterJumpCondition::Even,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JAO" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::A,
            RegisterJumpCondition::Odd,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J1N" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::Negative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J1Z" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::Zero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J1P" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::Positive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J1NN" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::NonNegative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J1NZ" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::NonZero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J1NP" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(1),
            RegisterJumpCondition::NonPositive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J2N" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::Negative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J2Z" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::Zero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J2P" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::Positive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J2NN" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::NonNegative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J2NZ" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::NonZero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J2NP" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(2),
            RegisterJumpCondition::NonPositive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J3N" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::Negative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J3Z" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::Zero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J3P" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::Positive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J3NN" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::NonNegative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J3NZ" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::NonZero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J3NP" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(3),
            RegisterJumpCondition::NonPositive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J4N" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::Negative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J4Z" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::Zero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J4P" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::Positive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J4NN" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::NonNegative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J4NZ" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::NonZero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J4NP" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(4),
            RegisterJumpCondition::NonPositive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J5N" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::Negative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J5Z" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::Zero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J5P" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::Positive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J5NN" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::NonNegative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J5NZ" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::NonZero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J5NP" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(5),
            RegisterJumpCondition::NonPositive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J6N" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::Negative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J6Z" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::Zero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J6P" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::Positive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J6NN" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::NonNegative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J6NZ" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::NonZero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "J6NP" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::I(6),
            RegisterJumpCondition::NonPositive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JXN" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::X,
            RegisterJumpCondition::Negative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JXZ" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::X,
            RegisterJumpCondition::Zero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JXP" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::X,
            RegisterJumpCondition::Positive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JXNN" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::X,
            RegisterJumpCondition::NonNegative,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JXNZ" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::X,
            RegisterJumpCondition::NonZero,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JXNP" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::X,
            RegisterJumpCondition::NonPositive,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JXE" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::X,
            RegisterJumpCondition::Even,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "JXO" => register_jump_instruction(
            operand_text,
            RegisterJumpTarget::X,
            RegisterJumpCondition::Odd,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "INCA" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::A,
            AddrTransferMode::Inc,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "DECA" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::A,
            AddrTransferMode::Dec,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENTA" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::A,
            AddrTransferMode::Ent,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENNA" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::A,
            AddrTransferMode::Enn,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "INC1" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(1),
            AddrTransferMode::Inc,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "DEC1" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(1),
            AddrTransferMode::Dec,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENT1" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(1),
            AddrTransferMode::Ent,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENN1" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(1),
            AddrTransferMode::Enn,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "INC2" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(2),
            AddrTransferMode::Inc,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "DEC2" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(2),
            AddrTransferMode::Dec,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENT2" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(2),
            AddrTransferMode::Ent,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENN2" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(2),
            AddrTransferMode::Enn,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "INC3" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(3),
            AddrTransferMode::Inc,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "DEC3" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(3),
            AddrTransferMode::Dec,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENT3" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(3),
            AddrTransferMode::Ent,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENN3" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(3),
            AddrTransferMode::Enn,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "INC4" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(4),
            AddrTransferMode::Inc,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "DEC4" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(4),
            AddrTransferMode::Dec,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENT4" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(4),
            AddrTransferMode::Ent,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENN4" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(4),
            AddrTransferMode::Enn,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "INC5" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(5),
            AddrTransferMode::Inc,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "DEC5" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(5),
            AddrTransferMode::Dec,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENT5" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(5),
            AddrTransferMode::Ent,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENN5" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(5),
            AddrTransferMode::Enn,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "INC6" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(6),
            AddrTransferMode::Inc,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "DEC6" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(6),
            AddrTransferMode::Dec,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENT6" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(6),
            AddrTransferMode::Ent,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENN6" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::I(6),
            AddrTransferMode::Enn,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "INCX" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::X,
            AddrTransferMode::Inc,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "DECX" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::X,
            AddrTransferMode::Dec,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENTX" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::X,
            AddrTransferMode::Ent,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "ENNX" => addr_transfer_instruction(
            operand_text,
            AddrTransferTarget::X,
            AddrTransferMode::Enn,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "CMPA" => compare_instruction(
            operand_text,
            CompareTarget::A,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "CMP1" => compare_instruction(
            operand_text,
            CompareTarget::I(1),
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "CMP2" => compare_instruction(
            operand_text,
            CompareTarget::I(2),
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "CMP3" => compare_instruction(
            operand_text,
            CompareTarget::I(3),
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "CMP4" => compare_instruction(
            operand_text,
            CompareTarget::I(4),
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "CMP5" => compare_instruction(
            operand_text,
            CompareTarget::I(5),
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "CMP6" => compare_instruction(
            operand_text,
            CompareTarget::I(6),
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        "CMPX" => compare_instruction(
            operand_text,
            CompareTarget::X,
            ctx,
            literal_entries,
            next_literal_addr,
        ),
        _ => Err(asm_syntax(
            ctx.line_no,
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

#[derive(Debug, Clone, Copy)]
struct EvaluatedOperand {
    address: i16,
    index: u8,
    field: u8,
    has_field: bool,
}

fn operand_instruction<F>(
    operand_text: &str,
    default_field: u8,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
    builder: F,
) -> Result<Instruction, AssemblerError>
where
    F: FnOnce(OperandSpec) -> Instruction,
{
    let op = parse_operand_value(
        operand_text,
        default_field,
        true,
        true,
        ctx,
        literal_entries,
        next_literal_addr,
    )?;
    Ok(builder(OperandSpec {
        addr: AddressSpec {
            address: op.address,
            index: op.index,
        },
        field: op.field,
    }))
}

fn fixed_address_instruction<F>(
    operand_text: &str,
    fixed_field: u8,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
    builder: F,
) -> Result<Instruction, AssemblerError>
where
    F: FnOnce(AddressSpec) -> Instruction,
{
    let op = parse_operand_value(
        operand_text,
        fixed_field,
        true,
        true,
        ctx,
        literal_entries,
        next_literal_addr,
    )?;
    if op.has_field {
        return Err(asm_syntax(
            ctx.line_no,
            "field cannot be overridden for this mnemonic",
        ));
    }
    Ok(builder(AddressSpec {
        address: op.address,
        index: op.index,
    }))
}

fn shift_instruction(
    operand_text: &str,
    mode: ShiftMode,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<Instruction, AssemblerError> {
    fixed_address_instruction(
        operand_text,
        0,
        ctx,
        literal_entries,
        next_literal_addr,
        |addr| Instruction::Shift { addr, mode },
    )
}

fn load_instruction(
    operand_text: &str,
    target: LoadTarget,
    negate: bool,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<Instruction, AssemblerError> {
    operand_instruction(
        operand_text,
        5,
        ctx,
        literal_entries,
        next_literal_addr,
        |operand| Instruction::Load {
            target,
            negate,
            operand,
        },
    )
}

fn store_instruction(
    operand_text: &str,
    source: StoreSource,
    default_field: u8,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<Instruction, AssemblerError> {
    operand_instruction(
        operand_text,
        default_field,
        ctx,
        literal_entries,
        next_literal_addr,
        |operand| Instruction::Store { source, operand },
    )
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
    kind: IoKind,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<Instruction, AssemblerError> {
    let op = parse_operand_value(
        operand_text,
        0,
        false,
        false,
        ctx,
        literal_entries,
        next_literal_addr,
    )?;
    let addr = AddressSpec {
        address: op.address,
        index: op.index,
    };
    Ok(match kind {
        IoKind::Jbus => Instruction::Jbus {
            addr,
            unit: op.field,
        },
        IoKind::Ioc => Instruction::Ioc {
            addr,
            unit: op.field,
        },
        IoKind::In => Instruction::In {
            addr,
            unit: op.field,
        },
        IoKind::Out => Instruction::Out {
            addr,
            unit: op.field,
        },
        IoKind::Jred => Instruction::Jred {
            addr,
            unit: op.field,
        },
    })
}

fn jump_instruction(
    operand_text: &str,
    cond: JumpCondition,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<Instruction, AssemblerError> {
    fixed_address_instruction(
        operand_text,
        0,
        ctx,
        literal_entries,
        next_literal_addr,
        |addr| Instruction::Jump { addr, cond },
    )
}

fn register_jump_instruction(
    operand_text: &str,
    target: RegisterJumpTarget,
    cond: RegisterJumpCondition,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<Instruction, AssemblerError> {
    fixed_address_instruction(
        operand_text,
        0,
        ctx,
        literal_entries,
        next_literal_addr,
        |addr| Instruction::RegisterJump { addr, target, cond },
    )
}

fn addr_transfer_instruction(
    operand_text: &str,
    target: AddrTransferTarget,
    mode: AddrTransferMode,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<Instruction, AssemblerError> {
    fixed_address_instruction(
        operand_text,
        0,
        ctx,
        literal_entries,
        next_literal_addr,
        |addr| Instruction::AddrTransfer { addr, target, mode },
    )
}

fn compare_instruction(
    operand_text: &str,
    target: CompareTarget,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<Instruction, AssemblerError> {
    operand_instruction(
        operand_text,
        5,
        ctx,
        literal_entries,
        next_literal_addr,
        |operand| Instruction::Compare { target, operand },
    )
}

/// Parses and evaluates MIX operand parts into encoded fields.
fn parse_operand_value(
    operand_text: &str,
    default_field: u8,
    field_is_fspec: bool,
    allow_literal_address: bool,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<EvaluatedOperand, AssemblerError> {
    // Operand grammar handled here:
    //   address[,index][(field)]
    if operand_text.is_empty() {
        return Ok(EvaluatedOperand {
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
                ctx.line_no,
                "field specification must end with `)`",
            ));
        }
        let close_idx = core.len() - 1;
        if open_idx >= close_idx {
            return Err(asm_syntax(ctx.line_no, "empty field specification"));
        }
        let field_text = core[open_idx + 1..close_idx].trim();
        let value = eval_expression(
            field_text,
            &EvalContext {
                expression_text: field_text,
                allow_future_standalone: false,
                ..ctx.clone()
            },
            OperandComponent::Field,
        )?;
        field = if field_is_fspec {
            i64_to_field(value, ctx.line_no)?
        } else {
            i64_to_u6(value, ctx.line_no, "field")?
        };
        has_field = true;
        core = core[..open_idx].trim();
    }

    let (address_text, index_text) = if let Some(comma_idx) = core.find(',') {
        (&core[..comma_idx], Some(&core[comma_idx + 1..]))
    } else {
        (core, None)
    };

    let address_val = if address_text.trim().is_empty() {
        0
    } else {
        eval_address_value(
            address_text.trim(),
            allow_literal_address,
            ctx,
            literal_entries,
            next_literal_addr,
        )?
    };

    let index = match index_text {
        Some(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Err(asm_syntax(
                    ctx.line_no,
                    "missing index expression after `,`",
                ));
            }
            let value = eval_expression(
                trimmed,
                &EvalContext {
                    expression_text: trimmed,
                    allow_future_standalone: false,
                    ..ctx.clone()
                },
                OperandComponent::Index,
            )?;
            i64_to_index(value, ctx.line_no)?
        }
        None => 0,
    };

    Ok(EvaluatedOperand {
        address: i64_to_i16(address_val, ctx.line_no, "address")?,
        index,
        field,
        has_field,
    })
}

/// Evaluates the address component, including literal pool allocation.
fn eval_address_value(
    address_text: &str,
    allow_literal_address: bool,
    ctx: &mut EvalContext<'_>,
    literal_entries: &mut Vec<LiteralEntry>,
    next_literal_addr: &mut i64,
) -> Result<i64, AssemblerError> {
    // Literal constants in address position allocate pool entries immediately
    // and return their assigned address.
    if allow_literal_address && is_literal_constant(address_text) {
        ensure_location_in_memory(*next_literal_addr, ctx.line_no)?;
        let inner = address_text[1..address_text.len() - 1].trim().to_owned();
        literal_entries.push(LiteralEntry {
            addr: *next_literal_addr,
            wexpr: inner,
            line_no: ctx.line_no,
            order: ctx.order,
        });
        let out = *next_literal_addr;
        *next_literal_addr += 1;
        return Ok(out);
    }

    eval_expression(
        address_text,
        &EvalContext {
            expression_text: address_text,
            allow_future_standalone: is_standalone_symbol_expr(address_text),
            ..ctx.clone()
        },
        OperandComponent::Address,
    )
}

/// Evaluates a MIX w-expression (`expr[(f)]` terms separated by commas).
pub(crate) fn eval_w_expression(
    text: &str,
    ctx: &EvalContext<'_>,
    component: OperandComponent,
) -> Result<i64, AssemblerError> {
    // A w-expression is a comma-separated sequence of terms, each optionally
    // carrying its own field spec. Terms are merged by field stores.
    let mut acc = MixWord::ZERO;
    for term in split_top_level_commas(text) {
        let (exp_text, fspec_opt) = split_wexpr_term(term, ctx.line_no)?;
        let exp_value = eval_expression(
            exp_text,
            &EvalContext {
                expression_text: exp_text,
                allow_future_standalone: false,
                ..ctx.clone()
            },
            component,
        )?;
        let source = MixWord::from_signed(exp_value, DEFAULT_BYTE_SIZE);

        let fspec = if let Some(fexpr) = fspec_opt {
            let fval = eval_expression(
                fexpr,
                &EvalContext {
                    expression_text: fexpr,
                    allow_future_standalone: false,
                    ..ctx.clone()
                },
                component,
            )?;
            i64_to_field(fval, ctx.line_no)?
        } else {
            5
        };

        acc.store_field(fspec, source)?;
    }
    Ok(acc.as_signed_i64(DEFAULT_BYTE_SIZE))
}

/// Evaluates a scalar expression used by operands and directives.
fn eval_expression(
    text: &str,
    ctx: &EvalContext<'_>,
    _component: OperandComponent,
) -> Result<i64, AssemblerError> {
    // Simple left-to-right expression parser used by operands/directives.
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(asm_syntax(ctx.line_no, "empty expression"));
    }

    let mut parser = ExprParser {
        input: trimmed,
        pos: 0,
        ctx,
    };
    let mut value = parser.parse_signed_atom()?;
    while let Some(op) = parser.parse_binary_op()? {
        let rhs = parser.parse_signed_atom()?;
        value = apply_binary_op(value, rhs, op, ctx.line_no)?;
    }

    if parser.pos != parser.input.len() {
        return Err(asm_syntax(
            ctx.line_no,
            &format!(
                "invalid expression near `{}`",
                &parser.input[parser.pos..]
            ),
        ));
    }

    Ok(value)
}

fn apply_binary_op(
    lhs: i64,
    rhs: i64,
    op: BinaryOp,
    line_no: usize,
) -> Result<i64, AssemblerError> {
    let overflow = || asm_syntax(line_no, "integer overflow in expression");
    match op {
        BinaryOp::Add => lhs.checked_add(rhs).ok_or_else(overflow),
        BinaryOp::Sub => lhs.checked_sub(rhs).ok_or_else(overflow),
        BinaryOp::Mul => lhs.checked_mul(rhs).ok_or_else(overflow),
        BinaryOp::Div => {
            if rhs == 0 {
                return Err(asm_syntax(
                    line_no,
                    "division by zero in expression",
                ));
            }
            lhs.checked_div(rhs).ok_or_else(overflow)
        }
        BinaryOp::DivLong => {
            if rhs == 0 {
                return Err(asm_syntax(
                    line_no,
                    "division by zero in expression",
                ));
            }
            let wide = MixWord::from_signed(lhs, DEFAULT_BYTE_SIZE)
                .magnitude(DEFAULT_BYTE_SIZE)
                * i64::from(DEFAULT_BYTE_SIZE).pow(5);
            wide.checked_div(rhs).ok_or_else(overflow)
        }
        BinaryOp::Fspec => lhs
            .checked_mul(8)
            .and_then(|v| v.checked_add(rhs))
            .ok_or_else(overflow),
    }
}

#[derive(Debug, Clone, Copy)]
enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    DivLong,
    Fspec,
}

struct ExprParser<'a, 'b> {
    input: &'a str,
    pos: usize,
    ctx: &'b EvalContext<'b>,
}

impl ExprParser<'_, '_> {
    // Signed atom parser allows repeated unary +/- prefixes.
    fn parse_signed_atom(&mut self) -> Result<i64, AssemblerError> {
        let mut sign = 1_i64;
        while self.peek_char() == Some('+') || self.peek_char() == Some('-') {
            if self.next_char() == Some('-') {
                sign = -sign;
            }
        }
        let atom = self.parse_atom()?;
        sign.checked_mul(atom).ok_or_else(|| {
            asm_syntax(self.ctx.line_no, "integer overflow in expression")
        })
    }

    fn parse_atom(&mut self) -> Result<i64, AssemblerError> {
        match self.peek_char() {
            Some('*') => {
                self.pos += 1;
                Ok(self.ctx.location)
            }
            Some(ch) if ch.is_ascii_digit() => {
                let token =
                    self.take_while(|c| c.is_ascii_alphanumeric()).to_owned();
                if token.chars().all(|c| c.is_ascii_digit()) {
                    token.parse::<i64>().map_err(|_| {
                        asm_syntax(
                            self.ctx.line_no,
                            &format!("invalid number `{token}`"),
                        )
                    })
                } else {
                    self.resolve_symbol(&token)
                }
            }
            Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {
                let token = self
                    .take_while(|c| c.is_ascii_alphanumeric() || c == '_')
                    .to_owned();
                self.resolve_symbol(&token)
            }
            _ => Err(asm_syntax(self.ctx.line_no, "invalid expression atom")),
        }
    }

    fn resolve_symbol(&self, raw: &str) -> Result<i64, AssemblerError> {
        let symbol = raw.to_ascii_uppercase();
        self.ctx.symbols.resolve(
            &symbol,
            self.ctx.order,
            self.ctx.allow_future_standalone,
            self.ctx.line_no,
        )
    }

    fn parse_binary_op(&mut self) -> Result<Option<BinaryOp>, AssemblerError> {
        let Some(ch) = self.peek_char() else {
            return Ok(None);
        };
        match ch {
            '+' => {
                self.pos += 1;
                Ok(Some(BinaryOp::Add))
            }
            '-' => {
                self.pos += 1;
                Ok(Some(BinaryOp::Sub))
            }
            '*' => {
                self.pos += 1;
                Ok(Some(BinaryOp::Mul))
            }
            ':' => {
                self.pos += 1;
                Ok(Some(BinaryOp::Fspec))
            }
            '/' => {
                self.pos += 1;
                if self.peek_char() == Some('/') {
                    self.pos += 1;
                    Ok(Some(BinaryOp::DivLong))
                } else {
                    Ok(Some(BinaryOp::Div))
                }
            }
            _ => Err(asm_syntax(
                self.ctx.line_no,
                &format!("invalid operator `{ch}` in expression"),
            )),
        }
    }

    fn take_while<F>(&mut self, mut pred: F) -> &str
    where
        F: FnMut(char) -> bool,
    {
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if pred(ch) {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        &self.input[start..self.pos]
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn next_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }
}

fn split_top_level_commas(text: &str) -> Vec<&str> {
    // Split by commas not nested inside parentheses.
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in text.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(text[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }
    out.push(text[start..].trim());
    out
}

fn split_wexpr_term<'a>(
    term: &'a str,
    line_no: usize,
) -> Result<(&'a str, Option<&'a str>), AssemblerError> {
    // Splits `expr(fspec)` into `(expr, Some(fspec))`.
    if term.is_empty() {
        return Err(asm_syntax(line_no, "empty term in w-expression"));
    }
    if let Some(open_idx) = term.find('(') {
        if !term.ends_with(')') {
            return Err(asm_syntax(line_no, "invalid w-expression term"));
        }
        let expr = term[..open_idx].trim();
        let fexpr = term[open_idx + 1..term.len() - 1].trim();
        if expr.is_empty() || fexpr.is_empty() {
            return Err(asm_syntax(line_no, "invalid w-expression term"));
        }
        Ok((expr, Some(fexpr)))
    } else {
        Ok((term.trim(), None))
    }
}

fn i64_to_i16(
    value: i64,
    line_no: usize,
    what: &str,
) -> Result<i16, AssemblerError> {
    i16::try_from(value).map_err(|_| {
        asm_syntax(line_no, &format!("{what} `{value}` out of range"))
    })
}

fn i64_to_index(value: i64, line_no: usize) -> Result<u8, AssemblerError> {
    if !(0..=6).contains(&value) {
        return Err(asm_syntax(
            line_no,
            &format!("index register `{value}` out of range"),
        ));
    }
    Ok(value as u8)
}

fn i64_to_field(value: i64, line_no: usize) -> Result<u8, AssemblerError> {
    if !(0..=63).contains(&value) {
        return Err(asm_syntax(
            line_no,
            &format!("field `{value}` out of range"),
        ));
    }
    let packed = value as u8;
    let left = packed / 8;
    let right = packed % 8;
    if left > right || right > 5 {
        return Err(asm_syntax(
            line_no,
            &format!("invalid field specification `{value}`"),
        ));
    }
    Ok(packed)
}

fn i64_to_u6(
    value: i64,
    line_no: usize,
    what: &str,
) -> Result<u8, AssemblerError> {
    if !(0..=63).contains(&value) {
        return Err(asm_syntax(
            line_no,
            &format!("{what} `{value}` out of range"),
        ));
    }
    Ok(value as u8)
}

fn is_literal_constant(text: &str) -> bool {
    text.starts_with('=') && text.ends_with('=') && text.len() >= 2
}

fn is_standalone_symbol_expr(text: &str) -> bool {
    let mut s = text.trim();
    if s.starts_with('+') || s.starts_with('-') {
        s = &s[1..];
    }
    !s.is_empty()
        && s.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && !s.chars().all(|ch| ch.is_ascii_digit())
}
