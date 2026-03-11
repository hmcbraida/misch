use misch_core::{MixError, assemble};
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

const WORD_BYTES: usize = 5;
const MIX_BASE: i64 = 64;
const SPACE_CODE: u8 = 0;

const MIX_CHAR_TABLE: [&str; 56] = [
    " ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "Δ", "J", "K", "L", "M",
    "N", "O", "P", "Q", "R", "Σ", "Π", "S", "T", "U", "V", "W", "X", "Y", "Z",
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ".", ",", "(", ")", "+",
    "-", "*", "/", "=", "$", "<", ">", "@", ";", ":", "'",
];

#[derive(Debug)]
struct Config {
    assembly_path: PathBuf,
    paper_tape_path: Option<PathBuf>,
    paper_tape_unit: u8,
    paper_tape_block_size: usize,
    line_writer_unit: u8,
    line_writer_block_size: usize,
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Io(io::Error),
    Mix(MixError),
    Assembler(misch_core::AssemblerError),
    InvalidMixCharacter(char),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Mix(err) => write!(f, "{err}"),
            Self::Assembler(err) => write!(f, "{err}"),
            Self::InvalidMixCharacter(ch) => {
                write!(f, "input contains unsupported MIX character: `{ch}`")
            }
        }
    }
}

impl Error for CliError {}

impl From<io::Error> for CliError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<MixError> for CliError {
    fn from(value: MixError) -> Self {
        Self::Mix(value)
    }
}

impl From<misch_core::AssemblerError> for CliError {
    fn from(value: misch_core::AssemblerError) -> Self {
        Self::Assembler(value)
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), CliError> {
    let config = parse_args(env::args().skip(1))?;
    let source = fs::read_to_string(&config.assembly_path)?;
    let mut state = assemble(&source)?;

    if let Some(path) = config.paper_tape_path {
        let content = fs::read_to_string(path)?;
        let words = encode_paper_tape_words(&content)?;
        let block_size = config.paper_tape_block_size;
        let mut cursor = 0usize;
        state.attach_input_callback(
            config.paper_tape_unit,
            block_size,
            move || {
                let mut block = Vec::with_capacity(block_size);
                for _ in 0..block_size {
                    if cursor < words.len() {
                        block.push(words[cursor]);
                        cursor += 1;
                    } else {
                        block.push(0);
                    }
                }
                Ok(block)
            },
        )?;
    }

    state.attach_output_callback(
        config.line_writer_unit,
        config.line_writer_block_size,
        move |block| {
            let mut out = String::new();
            for &word in block {
                out.push_str(&decode_word_to_text(word));
            }
            print!("{out}");
            let _ = io::stdout().flush();
            Ok(())
        },
    )?;

    while !state.is_halted() {
        state.advance_state()?;
    }

    println!();
    Ok(())
}

fn parse_args<I>(args: I) -> Result<Config, CliError>
where
    I: Iterator<Item = String>,
{
    let mut args = args.peekable();
    let mut assembly_path: Option<PathBuf> = None;
    let mut paper_tape_path: Option<PathBuf> = None;
    let mut paper_tape_unit: u8 = 16;
    let mut paper_tape_block_size: usize = 1;
    let mut line_writer_unit: u8 = 18;
    let mut line_writer_block_size: usize = 1;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--paper-tape" => {
                let value = args.next().ok_or_else(|| {
                    CliError::Usage(
                        "missing value for --paper-tape".to_string(),
                    )
                })?;
                paper_tape_path = Some(PathBuf::from(value));
            }
            "--paper-tape-unit" => {
                let value = args.next().ok_or_else(|| {
                    CliError::Usage(
                        "missing value for --paper-tape-unit".to_string(),
                    )
                })?;
                paper_tape_unit = parse_u8_arg("--paper-tape-unit", &value)?;
            }
            "--paper-tape-block-size" => {
                let value = args.next().ok_or_else(|| {
                    CliError::Usage(
                        "missing value for --paper-tape-block-size".to_string(),
                    )
                })?;
                paper_tape_block_size =
                    parse_usize_arg("--paper-tape-block-size", &value)?;
            }
            "--line-writer-unit" => {
                let value = args.next().ok_or_else(|| {
                    CliError::Usage(
                        "missing value for --line-writer-unit".to_string(),
                    )
                })?;
                line_writer_unit = parse_u8_arg("--line-writer-unit", &value)?;
            }
            "--line-writer-block-size" => {
                let value = args.next().ok_or_else(|| {
                    CliError::Usage(
                        "missing value for --line-writer-block-size"
                            .to_string(),
                    )
                })?;
                line_writer_block_size =
                    parse_usize_arg("--line-writer-block-size", &value)?;
            }
            "-h" | "--help" => {
                return Err(CliError::Usage(help_text().to_string()));
            }
            _ if arg.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown option `{arg}`")));
            }
            _ => {
                if assembly_path.is_none() {
                    assembly_path = Some(PathBuf::from(arg));
                } else {
                    return Err(CliError::Usage(format!(
                        "unexpected positional argument `{arg}`"
                    )));
                }
            }
        }
    }

    let assembly_path = assembly_path.ok_or_else(|| {
        CliError::Usage(format!(
            "missing assembly file path\n\n{}",
            help_text()
        ))
    })?;

    Ok(Config {
        assembly_path,
        paper_tape_path,
        paper_tape_unit,
        paper_tape_block_size,
        line_writer_unit,
        line_writer_block_size,
    })
}

fn parse_u8_arg(name: &str, value: &str) -> Result<u8, CliError> {
    value.parse::<u8>().map_err(|_| {
        CliError::Usage(format!("invalid value for {name}: `{value}`"))
    })
}

fn parse_usize_arg(name: &str, value: &str) -> Result<usize, CliError> {
    let parsed = value.parse::<usize>().map_err(|_| {
        CliError::Usage(format!("invalid value for {name}: `{value}`"))
    })?;
    if parsed == 0 {
        return Err(CliError::Usage(format!("{name} must be greater than 0")));
    }
    Ok(parsed)
}

fn help_text() -> &'static str {
    "Usage: misch-cli <assembly-file> [options]\n\nOptions:\n  --paper-tape <path>             UTF-8 text input for paper tape\n  --paper-tape-unit <unit>        MIX input unit (default: 16)\n  --paper-tape-block-size <n>     Words per paper tape read (default: 1)\n  --line-writer-unit <unit>       MIX output unit for stdout (default: 18)\n  --line-writer-block-size <n>    Words per line writer write (default: 1)"
}

fn encode_paper_tape_words(text: &str) -> Result<Vec<i64>, CliError> {
    let mut codes = Vec::new();
    for raw_char in text.chars() {
        if raw_char == '\n' || raw_char == '\r' {
            continue;
        }
        let ch = if raw_char.is_ascii_lowercase() {
            raw_char.to_ascii_uppercase()
        } else {
            raw_char
        };
        let code =
            mix_code_for_char(ch).ok_or(CliError::InvalidMixCharacter(ch))?;
        codes.push(code);
    }

    let mut words = Vec::new();
    for chunk in codes.chunks(WORD_BYTES) {
        let mut word = 0_i64;
        for i in 0..WORD_BYTES {
            let code = chunk.get(i).copied().unwrap_or(SPACE_CODE);
            word = word * MIX_BASE + i64::from(code);
        }
        words.push(word);
    }
    Ok(words)
}

fn decode_word_to_text(word: i64) -> String {
    let mut value = word.abs();
    let mut codes = [0u8; WORD_BYTES];
    for i in (0..WORD_BYTES).rev() {
        codes[i] = (value % MIX_BASE) as u8;
        value /= MIX_BASE;
    }

    let mut out = String::new();
    for code in codes {
        let symbol = MIX_CHAR_TABLE
            .get(usize::from(code))
            .copied()
            .unwrap_or("?");
        out.push_str(symbol);
    }
    out
}

fn mix_code_for_char(ch: char) -> Option<u8> {
    match ch {
        ' ' => Some(0),
        'A' => Some(1),
        'B' => Some(2),
        'C' => Some(3),
        'D' => Some(4),
        'E' => Some(5),
        'F' => Some(6),
        'G' => Some(7),
        'H' => Some(8),
        'I' => Some(9),
        'Δ' => Some(10),
        'J' => Some(11),
        'K' => Some(12),
        'L' => Some(13),
        'M' => Some(14),
        'N' => Some(15),
        'O' => Some(16),
        'P' => Some(17),
        'Q' => Some(18),
        'R' => Some(19),
        'Σ' => Some(20),
        'Π' => Some(21),
        'S' => Some(22),
        'T' => Some(23),
        'U' => Some(24),
        'V' => Some(25),
        'W' => Some(26),
        'X' => Some(27),
        'Y' => Some(28),
        'Z' => Some(29),
        '0' => Some(30),
        '1' => Some(31),
        '2' => Some(32),
        '3' => Some(33),
        '4' => Some(34),
        '5' => Some(35),
        '6' => Some(36),
        '7' => Some(37),
        '8' => Some(38),
        '9' => Some(39),
        '.' => Some(40),
        ',' => Some(41),
        '(' => Some(42),
        ')' => Some(43),
        '+' => Some(44),
        '-' => Some(45),
        '*' => Some(46),
        '/' => Some(47),
        '=' => Some(48),
        '$' => Some(49),
        '<' => Some(50),
        '>' => Some(51),
        '@' => Some(52),
        ';' => Some(53),
        ':' => Some(54),
        '\'' => Some(55),
        _ => None,
    }
}
