use misch_core::{
    MixCharError, MixError, assemble, decode_word_to_text, encode_text_to_words,
};
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

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
    MixChar(MixCharError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "{message}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Mix(err) => write!(f, "{err}"),
            Self::Assembler(err) => write!(f, "{err}"),
            Self::MixChar(err) => write!(f, "{err}"),
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

impl From<MixCharError> for CliError {
    fn from(value: MixCharError) -> Self {
        Self::MixChar(value)
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
        let words = encode_text_to_words(&content)?;
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
        {
            let mut emitted_chars = 0usize;
            move |block| {
                for &word in block {
                    let text = decode_word_to_text(word);
                    for ch in text.chars() {
                        print!("{ch}");
                        emitted_chars += 1;
                        if emitted_chars % 100 == 0 {
                            print!("\n");
                        }
                    }
                }
                let _ = io::stdout().flush();
                Ok(())
            }
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
