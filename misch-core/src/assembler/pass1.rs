use super::parse::{Directive, LineKind, ParsedLine};
use super::pass2::eval_w_expression;
use super::{
    AssemblerError, EvalContext, OperandComponent, asm_syntax,
    ensure_location_in_memory,
};
use std::collections::HashMap;

#[derive(Debug, Clone)]
/// Represents a value targeting a particular memory location.
///
/// Evolves from [`ParsedLine`].
pub(crate) struct AsmItem {
    pub(crate) line_no: usize,
    pub(crate) order: usize,
    pub(crate) location: i64,
    pub(crate) kind: ItemKind,
}

#[derive(Debug, Clone)]
/// Encodable item kinds carried from pass 1 to pass 2.
pub(crate) enum ItemKind {
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
    pub(crate) order: usize,
}

#[derive(Debug, Clone)]
/// Global and local symbol spaces.
///
/// - [`SymbolTables::globals`] stores ordinary labels.
/// - [`SymbolTables::locals`] stores MIX local labels (`1H`..`9H`) as ordered
///   definition lists.
pub(crate) struct SymbolTables {
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
    pub(crate) fn define(
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
    pub(crate) fn resolve(
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
pub(crate) struct FirstPass {
    pub(crate) items: Vec<AsmItem>,
    pub(crate) symbols: SymbolTables,
    pub(crate) end_start: i64,
    pub(crate) literal_start: i64,
}

/// First assembly pass.
///
/// Responsibilities:
/// - define symbols (`LABEL`, `EQU`, local labels)
/// - maintain location counter (`ORIG`, emitted words)
/// - collect encodable items for pass 2
/// - capture start address from `END`
pub(crate) fn first_pass(
    lines: &[ParsedLine],
) -> Result<FirstPass, AssemblerError> {
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
