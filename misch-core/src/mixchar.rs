use std::fmt;

const WORD_BYTES: usize = 5;
const MIX_BASE: i64 = 64;
const SPACE_CODE: u8 = 0;

const MIX_CHAR_TABLE: [&str; 56] = [
    " ", "A", "B", "C", "D", "E", "F", "G", "H", "I", "Δ", "J", "K", "L", "M",
    "N", "O", "P", "Q", "R", "Σ", "Π", "S", "T", "U", "V", "W", "X", "Y", "Z",
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ".", ",", "(", ")", "+",
    "-", "*", "/", "=", "$", "<", ">", "@", ";", ":", "'",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixCharError {
    UnsupportedCharacter(char),
}

impl fmt::Display for MixCharError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCharacter(ch) => {
                write!(f, "input contains unsupported MIX character: `{ch}`")
            }
        }
    }
}

impl std::error::Error for MixCharError {}

pub fn encode_text_to_words(text: &str) -> Result<Vec<i64>, MixCharError> {
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
        let code = mix_code_for_char(ch)
            .ok_or(MixCharError::UnsupportedCharacter(ch))?;
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

pub fn decode_word_to_text(word: i64) -> String {
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

pub fn decode_words_to_text(words: &[i64]) -> String {
    let mut out = String::new();
    for &word in words {
        out.push_str(&decode_word_to_text(word));
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
