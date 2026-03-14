//! MIXAL assembler implementation.
//!
//! Exposes a method [`assemble`] which consumes a string representing a MIXAL
//! program and returns the [`MixState`] which would be assembled as a result.
//!
//! High-level procedure:
//! - Parse source text into a compact line model ([`ParsedLine`]).
//! - Run a first pass to build symbol tables and assign locations.
//! - Run a second pass to encode instructions/directives into [`MixState`] memory.
//!
//! The implementation intentionally keeps parsing and encoding separate:
//! pass 1 resolves layout/symbol definitions while pass 2 performs full operand
//! evaluation and instruction encoding.

use crate::instruction::{
    AddrTransferMode, AddrTransferTarget, AddressSpec, CompareTarget,
    Instruction, JumpCondition, LoadTarget, OperandSpec, RegisterJumpCondition,
    RegisterJumpTarget, ShiftMode, StoreSource,
};
use crate::mixchar::encode_text_to_words;
use crate::state::MixState;
use crate::word::MixWord;
use crate::{MixCharError, MixError};
use std::collections::HashMap;
use std::fmt;

const DEFAULT_BYTE_SIZE: u16 = 64;
const MIX_MEMORY_SIZE: i64 = 4000;

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

#[derive(Debug, Clone)]
/// One non-empty, non-comment logical source line.
struct ParsedLine {
    line_no: usize,
    order: usize,
    label: Option<String>,
    kind: LineKind,
}

#[derive(Debug, Clone)]
/// Parsed line category: instruction or assembler directive.
enum LineKind {
    Instruction {
        mnemonic: String,
        operand: String,
    },
    Directive {
        directive: Directive,
        operand: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Supported assembler directives.
enum Directive {
    Orig,
    Equ,
    Con,
    Alf,
    End,
}

#[derive(Debug, Clone)]
/// Represents a value targeting a particular memory location.
///
/// Evolves from [`ParsedLine`].
struct AsmItem {
    line_no: usize,
    order: usize,
    location: i64,
    kind: ItemKind,
}

#[derive(Debug, Clone)]
/// Encodable item kinds carried from pass 1 to pass 2.
enum ItemKind {
    Instruction { mnemonic: String, operand: String },
    Con { operand: String },
    Alf { operand: String },
}

#[derive(Debug, Clone, Copy)]
/// Symbol definition metadata used for resolution.
///
/// In the case of a location (non-EQU) symbol, this can be seen as something
/// which directly maps the source assembly to the target machine memory state:
/// mapping [`SymbolDef::order`] to [`SymbolDef::value`].
struct SymbolDef {
    value: i64,
    order: usize,
}

#[derive(Debug, Clone)]
/// Global and local symbol spaces.
///
/// - [`SymbolTables::globals`] stores ordinary labels.
/// - [`SymbolTables::locals`] stores MIX local labels (`1H`..`9H`) as ordered
///   definition lists.
struct SymbolTables {
    globals: HashMap<String, SymbolDef>,
    locals: HashMap<u8, Vec<SymbolDef>>,
}

impl SymbolTables {
    fn new() -> Self {
        Self {
            globals: HashMap::new(),
            locals: HashMap::new(),
        }
    }

    /// Defines a symbol at the point.
    ///
    /// This includes parsing it into either a local or a global symbol
    /// depending on if it matches the `nH` pattern.
    fn define(
        &mut self,
        label: &str,
        value: i64,
        order: usize,
        line_no: usize,
    ) -> Result<(), AssemblerError> {
        if let Some(local_digit) = local_h_digit(label) {
            self.locals
                .entry(local_digit)
                .or_default()
                .push(SymbolDef { value, order });
            return Ok(());
        }

        if self.globals.contains_key(label) {
            return Err(asm_syntax(
                line_no,
                &format!("symbol `{label}` is already defined"),
            ));
        }
        self.globals
            .insert(label.to_owned(), SymbolDef { value, order });
        Ok(())
    }

    /// Resolves the value of the given symbol.
    ///
    /// Includes detection of local (`nF`, `nB`) references.
    fn resolve(
        &self,
        symbol: &str,
        usage_order: usize,
        allow_future: bool,
        line_no: usize,
    ) -> Result<i64, AssemblerError> {
        if let Some((digit, flavor)) = local_symbol_ref(symbol) {
            let defs = self.locals.get(&digit).ok_or_else(|| {
                asm_syntax(
                    line_no,
                    &format!("undefined local symbol `{symbol}`"),
                )
            })?;
            let candidate = match flavor {
                'B' => defs.iter().rev().find(|d| d.order < usage_order),
                'F' => defs.iter().find(|d| d.order > usage_order),
                _ => None,
            };
            let def = candidate.ok_or_else(|| {
                asm_syntax(
                    line_no,
                    &format!("undefined local symbol `{symbol}`"),
                )
            })?;
            if def.order > usage_order && !allow_future {
                return Err(asm_syntax(
                    line_no,
                    &format!(
                        "future reference `{symbol}` is only allowed as standalone address"
                    ),
                ));
            }
            return Ok(def.value);
        }

        let def = self.globals.get(symbol).ok_or_else(|| {
            asm_syntax(line_no, &format!("undefined symbol `{symbol}`"))
        })?;
        if def.order > usage_order && !allow_future {
            return Err(asm_syntax(
                line_no,
                &format!(
                    "future reference `{symbol}` is only allowed as standalone address"
                ),
            ));
        }
        Ok(def.value)
    }
}

#[derive(Debug)]
/// State from pass 1, consumed by pass 2.
struct FirstPass {
    items: Vec<AsmItem>,
    symbols: SymbolTables,
    end_start: i64,
    literal_start: i64,
}

#[derive(Debug, Clone)]
/// Deferred literal allocated during operand parsing (e.g. `=5=`).
struct LiteralEntry {
    addr: i64,
    wexpr: String,
    line_no: usize,
    order: usize,
}

#[derive(Debug, Clone, Copy)]
/// Expression evaluation context to improve diagnostics and symbol rules.
enum OperandComponent {
    Address,
    Index,
    Field,
    WExpr,
}

#[derive(Debug, Clone)]
/// Shared context used when evaluating expressions and operands.
struct EvalContext<'a> {
    symbols: &'a SymbolTables,
    line_no: usize,
    order: usize,
    location: i64,
    allow_future_standalone: bool,
    expression_text: &'a str,
}

/// Assembles MIXAL source code into an initialized [`MixState`].
pub fn assemble(source: &str) -> Result<MixState, AssemblerError> {
    let parsed = parse_source(source)?;
    let first_pass = first_pass(&parsed)?;
    second_pass(&first_pass)
}

/// Parses source text into logical lines, removing comments/blank lines.
///
/// Parsing stops after the first `END`.
fn parse_source(source: &str) -> Result<Vec<ParsedLine>, AssemblerError> {
    let mut out = Vec::new();
    let mut saw_end = false;

    for (line_idx, raw_line) in source.lines().enumerate() {
        let line_no = line_idx + 1;
        if saw_end {
            continue;
        }

        let line = strip_hash_and_semicolon_comments(raw_line);
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('*') {
            continue;
        }

        let parsed = parse_logical_line(line, line_no, out.len())?;
        if matches!(
            parsed.kind,
            LineKind::Directive {
                directive: Directive::End,
                ..
            }
        ) {
            saw_end = true;
        }
        out.push(parsed);
    }

    if !saw_end {
        return Err(asm_syntax(1, "missing mandatory `END` directive"));
    }

    Ok(out)
}

/// Parses one logical line into `{label?, mnemonic/directive, operand}`.
fn parse_logical_line(
    line: &str,
    line_no: usize,
    order: usize,
) -> Result<ParsedLine, AssemblerError> {
    let (first, rest_after_first) = take_token(line).ok_or_else(|| {
        asm_syntax(line_no, "line must contain a mnemonic or directive")
    })?;

    let first_up = first.to_ascii_uppercase();
    let (label, mnemonic, rest) = if is_opcode_or_directive(&first_up) {
        (None, first_up, rest_after_first)
    } else {
        let (mnemonic_token, rest_after_mnemonic) =
            take_token(rest_after_first).ok_or_else(|| {
                asm_syntax(line_no, "missing mnemonic after label")
            })?;
        (
            Some(first_up),
            mnemonic_token.to_ascii_uppercase(),
            rest_after_mnemonic,
        )
    };

    let operand = parse_operand_text(&mnemonic, rest, line_no)?;

    let kind = if let Some(directive) = parse_directive(&mnemonic) {
        LineKind::Directive { directive, operand }
    } else {
        LineKind::Instruction { mnemonic, operand }
    };

    Ok(ParsedLine {
        line_no,
        order,
        label,
        kind,
    })
}

/// First assembly pass.
///
/// Responsibilities:
/// - define symbols (`LABEL`, `EQU`, local labels)
/// - maintain location counter (`ORIG`, emitted words)
/// - collect encodable items for pass 2
/// - capture start address from `END`
fn first_pass(lines: &[ParsedLine]) -> Result<FirstPass, AssemblerError> {
    // Pass 1 assigns absolute locations and records symbol definitions.
    // It does not encode instructions yet.
    let mut items = Vec::new();
    let mut symbols = SymbolTables::new();
    let mut location_counter = 0_i64;
    let mut end_start = None;

    for line in lines {
        match &line.kind {
            LineKind::Directive {
                directive: Directive::Equ,
                operand,
            } => {
                let label = line.label.as_ref().ok_or_else(|| {
                    asm_syntax(line.line_no, "`EQU` requires a label")
                })?;
                let eq_val = eval_w_expression(
                    operand,
                    &EvalContext {
                        symbols: &symbols,
                        line_no: line.line_no,
                        order: line.order,
                        location: location_counter,
                        allow_future_standalone: false,
                        expression_text: operand,
                    },
                    OperandComponent::WExpr,
                )?;
                symbols.define(label, eq_val, line.order, line.line_no)?;
            }
            _ => {
                if let Some(label) = line.label.as_deref() {
                    symbols.define(
                        label,
                        location_counter,
                        line.order,
                        line.line_no,
                    )?;
                }

                match &line.kind {
                    LineKind::Instruction { mnemonic, operand } => {
                        items.push(AsmItem {
                            line_no: line.line_no,
                            order: line.order,
                            location: location_counter,
                            kind: ItemKind::Instruction {
                                mnemonic: mnemonic.clone(),
                                operand: operand.clone(),
                            },
                        });
                        location_counter += 1;
                    }
                    LineKind::Directive {
                        directive: Directive::Orig,
                        operand,
                    } => {
                        let new_location = eval_w_expression(
                            operand,
                            &EvalContext {
                                symbols: &symbols,
                                line_no: line.line_no,
                                order: line.order,
                                location: location_counter,
                                allow_future_standalone: false,
                                expression_text: operand,
                            },
                            OperandComponent::WExpr,
                        )?;
                        ensure_location_in_memory(new_location, line.line_no)?;
                        location_counter = new_location;
                    }
                    LineKind::Directive {
                        directive: Directive::Con,
                        operand,
                    } => {
                        items.push(AsmItem {
                            line_no: line.line_no,
                            order: line.order,
                            location: location_counter,
                            kind: ItemKind::Con {
                                operand: operand.clone(),
                            },
                        });
                        location_counter += 1;
                    }
                    LineKind::Directive {
                        directive: Directive::Alf,
                        operand,
                    } => {
                        items.push(AsmItem {
                            line_no: line.line_no,
                            order: line.order,
                            location: location_counter,
                            kind: ItemKind::Alf {
                                operand: operand.clone(),
                            },
                        });
                        location_counter += 1;
                    }
                    LineKind::Directive {
                        directive: Directive::End,
                        operand,
                    } => {
                        let start = eval_w_expression(
                            operand,
                            &EvalContext {
                                symbols: &symbols,
                                line_no: line.line_no,
                                order: line.order,
                                location: location_counter,
                                allow_future_standalone: false,
                                expression_text: operand,
                            },
                            OperandComponent::WExpr,
                        )?;
                        ensure_location_in_memory(start, line.line_no)?;
                        end_start = Some(start);
                    }
                    LineKind::Directive {
                        directive: Directive::Equ,
                        ..
                    } => {}
                }
            }
        }
    }

    let end_start =
        end_start.ok_or_else(|| asm_syntax(1, "missing `END` directive"))?;

    Ok(FirstPass {
        items,
        symbols,
        end_start,
        literal_start: location_counter,
    })
}

/// Second assembly pass.
///
/// Encodes all first-pass items into memory and emits deferred literal words.
fn second_pass(first: &FirstPass) -> Result<MixState, AssemblerError> {
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

fn ensure_location_in_memory(
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

/// Parses the operand token for a mnemonic/directive.
///
/// `ALF` is special-cased to preserve quoted whitespace content.
fn parse_operand_text(
    mnemonic: &str,
    rest: &str,
    line_no: usize,
) -> Result<String, AssemblerError> {
    let trimmed = rest.trim_start();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    if mnemonic == "ALF" {
        if trimmed.starts_with('"') || trimmed.starts_with('\'') {
            let quote = trimmed.chars().next().unwrap();
            let end_idx = trimmed[1..].find(quote).ok_or_else(|| {
                asm_syntax(line_no, "unterminated quoted `ALF` operand")
            })? + 1;
            return Ok(trimmed[..=end_idx].to_owned());
        }
        let (token, _) = take_token(trimmed)
            .ok_or_else(|| asm_syntax(line_no, "`ALF` requires an operand"))?;
        return Ok(token.to_owned());
    }

    let (token, _) = take_token(trimmed)
        .ok_or_else(|| asm_syntax(line_no, "invalid operand"))?;
    Ok(token.to_owned())
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
fn eval_w_expression(
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

fn take_token(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let start_offset = input.len() - trimmed.len();
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let token = &trimmed[..end];
    let rest_idx = start_offset + end;
    Some((token, &input[rest_idx..]))
}

fn parse_directive(token: &str) -> Option<Directive> {
    match token {
        "ORIG" => Some(Directive::Orig),
        "EQU" => Some(Directive::Equ),
        "CON" => Some(Directive::Con),
        "ALF" => Some(Directive::Alf),
        "END" => Some(Directive::End),
        _ => None,
    }
}

fn is_opcode_or_directive(token: &str) -> bool {
    parse_directive(token).is_some() || is_opcode(token)
}

fn is_opcode(token: &str) -> bool {
    matches!(
        token,
        "NOP"
            | "ADD"
            | "SUB"
            | "MUL"
            | "DIV"
            | "NUM"
            | "CHAR"
            | "HLT"
            | "SLA"
            | "SRA"
            | "SLAX"
            | "SRAX"
            | "SLC"
            | "SRC"
            | "SLB"
            | "SRB"
            | "MOVE"
            | "LDA"
            | "LD1"
            | "LD2"
            | "LD3"
            | "LD4"
            | "LD5"
            | "LD6"
            | "LDX"
            | "LDAN"
            | "LD1N"
            | "LD2N"
            | "LD3N"
            | "LD4N"
            | "LD5N"
            | "LD6N"
            | "LDXN"
            | "STA"
            | "ST1"
            | "ST2"
            | "ST3"
            | "ST4"
            | "ST5"
            | "ST6"
            | "STX"
            | "STJ"
            | "STZ"
            | "JBUS"
            | "IOC"
            | "IN"
            | "OUT"
            | "JRED"
            | "JMP"
            | "JSJ"
            | "JOV"
            | "JNOV"
            | "JL"
            | "JE"
            | "JG"
            | "JGE"
            | "JNE"
            | "JLE"
            | "JAN"
            | "JAZ"
            | "JAP"
            | "JANN"
            | "JANZ"
            | "JANP"
            | "JAE"
            | "JAO"
            | "J1N"
            | "J1Z"
            | "J1P"
            | "J1NN"
            | "J1NZ"
            | "J1NP"
            | "J2N"
            | "J2Z"
            | "J2P"
            | "J2NN"
            | "J2NZ"
            | "J2NP"
            | "J3N"
            | "J3Z"
            | "J3P"
            | "J3NN"
            | "J3NZ"
            | "J3NP"
            | "J4N"
            | "J4Z"
            | "J4P"
            | "J4NN"
            | "J4NZ"
            | "J4NP"
            | "J5N"
            | "J5Z"
            | "J5P"
            | "J5NN"
            | "J5NZ"
            | "J5NP"
            | "J6N"
            | "J6Z"
            | "J6P"
            | "J6NN"
            | "J6NZ"
            | "J6NP"
            | "JXN"
            | "JXZ"
            | "JXP"
            | "JXNN"
            | "JXNZ"
            | "JXNP"
            | "JXE"
            | "JXO"
            | "INCA"
            | "DECA"
            | "ENTA"
            | "ENNA"
            | "INC1"
            | "DEC1"
            | "ENT1"
            | "ENN1"
            | "INC2"
            | "DEC2"
            | "ENT2"
            | "ENN2"
            | "INC3"
            | "DEC3"
            | "ENT3"
            | "ENN3"
            | "INC4"
            | "DEC4"
            | "ENT4"
            | "ENN4"
            | "INC5"
            | "DEC5"
            | "ENT5"
            | "ENN5"
            | "INC6"
            | "DEC6"
            | "ENT6"
            | "ENN6"
            | "INCX"
            | "DECX"
            | "ENTX"
            | "ENNX"
            | "CMPA"
            | "CMP1"
            | "CMP2"
            | "CMP3"
            | "CMP4"
            | "CMP5"
            | "CMP6"
            | "CMPX"
    )
}

fn strip_hash_and_semicolon_comments(line: &str) -> &str {
    let hash_idx = line.find('#');
    let semi_idx = line.find(';');
    match (hash_idx, semi_idx) {
        (Some(h), Some(s)) => &line[..h.min(s)],
        (Some(h), None) => &line[..h],
        (None, Some(s)) => &line[..s],
        (None, None) => line,
    }
}

fn local_h_digit(label: &str) -> Option<u8> {
    let bytes = label.as_bytes();
    if bytes.len() == 2 && (b'1'..=b'9').contains(&bytes[0]) && bytes[1] == b'H'
    {
        Some(bytes[0] - b'0')
    } else {
        None
    }
}

fn local_symbol_ref(symbol: &str) -> Option<(u8, char)> {
    let bytes = symbol.as_bytes();
    if bytes.len() == 2 && (b'1'..=b'9').contains(&bytes[0]) {
        let flavor = bytes[1] as char;
        if matches!(flavor, 'B' | 'F') {
            return Some((bytes[0] - b'0', flavor));
        }
    }
    None
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
