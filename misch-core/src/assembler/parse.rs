use super::{AssemblerError, asm_syntax};

pub(crate) struct ParsedLine {
    pub(crate) line_no: usize,
    pub(crate) order: usize,
    pub(crate) label: Option<String>,
    pub(crate) kind: LineKind,
}

#[derive(Debug, Clone)]
/// Parsed line category: instruction or assembler directive.
pub(crate) enum LineKind {
    /// Represents a MIX instruction.
    ///
    /// The operand may be an arbitrary w-expression.
    Instruction { mnemonic: String, operand: String },
    /// Represents an assembly directive, as opposed to an instruction.
    Directive {
        directive: Directive,
        operand: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Supported assembler directives.
pub(crate) enum Directive {
    Orig,
    Equ,
    Con,
    Alf,
    End,
}

/// Parses source text into logical lines, removing comments/blank lines.
///
/// Parsing stops after the first `END`.
pub(crate) fn parse_source(
    source: &str,
) -> Result<Vec<ParsedLine>, AssemblerError> {
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
