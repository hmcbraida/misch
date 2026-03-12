use crate::error::MixError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Sign bit used by MIX words and half-words.
pub(crate) enum Sign {
    /// Positive sign (`+`).
    Positive,
    /// Negative sign (`-`).
    Negative,
}

impl Sign {
    /// Returns the opposite sign.
    pub(crate) fn negate(self) -> Self {
        match self {
            Self::Positive => Self::Negative,
            Self::Negative => Self::Positive,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Result of a MIX compare operation.
pub(crate) enum Comparison {
    /// Left side is less than right side.
    Less,
    /// Left side equals right side.
    Equal,
    /// Left side is greater than right side.
    Greater,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// MIX full word: sign plus five bytes.
pub(crate) struct MixWord {
    pub(crate) sign: Sign,
    pub(crate) bytes: [u16; 5],
}

impl MixWord {
    /// Canonical positive zero word.
    pub(crate) const ZERO: Self = Self {
        sign: Sign::Positive,
        bytes: [0; 5],
    };

    /// Validates that each byte is `< byte_size`.
    pub(crate) fn validate(&self, byte_size: u16) -> Result<(), MixError> {
        for b in self.bytes {
            if b >= byte_size {
                return Err(MixError::ByteOutOfRange {
                    value: b,
                    byte_size,
                });
            }
        }
        Ok(())
    }

    /// Returns the unsigned magnitude represented by this word.
    pub(crate) fn magnitude(&self, byte_size: u16) -> i64 {
        let mut value = 0_i64;
        for b in self.bytes {
            value = value * i64::from(byte_size) + i64::from(b);
        }
        value
    }

    /// Returns the signed integer value represented by this word.
    pub(crate) fn as_signed_i64(&self, byte_size: u16) -> i64 {
        let mag = self.magnitude(byte_size);
        match self.sign {
            Sign::Positive => mag,
            Sign::Negative => -mag,
        }
    }

    /// Builds a MIX word from a signed integer, truncating to 5 bytes.
    pub(crate) fn from_signed(value: i64, byte_size: u16) -> Self {
        let sign = if value < 0 {
            Sign::Negative
        } else {
            Sign::Positive
        };
        let mut rem = value.unsigned_abs();
        let mut bytes = [0_u16; 5];
        let base = u64::from(byte_size);
        for idx in (0..5).rev() {
            bytes[idx] = (rem % base) as u16;
            rem /= base;
        }
        Self { sign, bytes }
    }

    /// Returns a copy with the provided sign.
    pub(crate) fn with_sign(self, sign: Sign) -> Self {
        Self { sign, ..self }
    }

    /// Returns a copy with inverted sign.
    pub(crate) fn negate(self) -> Self {
        Self {
            sign: self.sign.negate(),
            ..self
        }
    }

    /// Extracts left endpoint `L` from packed field `8 * L + R`.
    fn field_l(field: u8) -> u8 {
        field / 8
    }

    /// Extracts right endpoint `R` from packed field `8 * L + R`.
    fn field_r(field: u8) -> u8 {
        field % 8
    }

    /// Returns a word containing only the selected field slice.
    ///
    /// Field syntax follows MIX convention where `0` addresses the sign and
    /// `1..=5` address bytes.
    pub(crate) fn slice(self, field: u8) -> Result<Self, MixError> {
        let l = Self::field_l(field);
        let r = Self::field_r(field);
        if l > r || r > 5 {
            return Err(MixError::InvalidFieldSpec(field));
        }

        if l == 0 && r == 0 {
            return Ok(Self {
                sign: self.sign,
                bytes: [0; 5],
            });
        }

        let sign = if l == 0 { self.sign } else { Sign::Positive };
        let mut out = [0_u16; 5];
        let selected_start = if l == 0 { 1 } else { l };
        let selected_len = usize::from(r - selected_start + 1);
        let dst_start = 5 - selected_len;

        for k in 0..selected_len {
            let src_index = usize::from(selected_start) - 1 + k;
            out[dst_start + k] = self.bytes[src_index];
        }

        Ok(Self { sign, bytes: out })
    }

    /// Stores selected bytes/sign from `source` into `self`.
    ///
    /// Field semantics match MIX `ST*` instruction behavior.
    pub(crate) fn store_field(
        &mut self,
        field: u8,
        source: Self,
    ) -> Result<(), MixError> {
        let l = Self::field_l(field);
        let r = Self::field_r(field);
        if l > r || r > 5 {
            return Err(MixError::InvalidFieldSpec(field));
        }

        if l == 0 {
            self.sign = source.sign;
            if r == 0 {
                return Ok(());
            }
        }

        let byte_l = if l == 0 { 1 } else { l };
        let count = usize::from(r - byte_l + 1);
        let src_start = 5 - count;

        for k in 0..count {
            let dst_idx = usize::from(byte_l) - 1 + k;
            self.bytes[dst_idx] = source.bytes[src_start + k];
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// MIX half-word: sign plus two bytes.
pub(crate) struct MixHalfWord {
    pub(crate) sign: Sign,
    pub(crate) bytes: [u16; 2],
}

impl MixHalfWord {
    /// Canonical positive zero half-word.
    pub(crate) const ZERO: Self = Self {
        sign: Sign::Positive,
        bytes: [0; 2],
    };

    /// Returns the signed integer value represented by this half-word.
    pub(crate) fn as_signed_i32(&self, byte_size: u16) -> i32 {
        let mag = i32::from(self.bytes[0]) * i32::from(byte_size)
            + i32::from(self.bytes[1]);
        match self.sign {
            Sign::Positive => mag,
            Sign::Negative => -mag,
        }
    }

    /// Builds a half-word from a signed integer, truncating to 2 bytes.
    pub(crate) fn from_signed(value: i32, byte_size: u16) -> Self {
        let sign = if value < 0 {
            Sign::Negative
        } else {
            Sign::Positive
        };
        let mut rem = value.unsigned_abs();
        let base = u32::from(byte_size);
        let low = (rem % base) as u16;
        rem /= base;
        let high = (rem % base) as u16;
        Self {
            sign,
            bytes: [high, low],
        }
    }

    /// Extracts a half-word from the low two bytes of a full word.
    pub(crate) fn from_word(word: MixWord) -> Self {
        Self {
            sign: word.sign,
            bytes: [word.bytes[3], word.bytes[4]],
        }
    }

    /// Expands this half-word into a full word using low-byte placement.
    pub(crate) fn to_word(self) -> MixWord {
        MixWord {
            sign: self.sign,
            bytes: [0, 0, 0, self.bytes[0], self.bytes[1]],
        }
    }

    /// Returns whether the value sign is negative.
    pub(crate) fn is_negative(self) -> bool {
        self.sign == Sign::Negative
    }

    /// Returns whether the magnitude is zero.
    pub(crate) fn is_zero(self) -> bool {
        self.bytes == [0, 0]
    }

    /// Returns whether the value is strictly positive.
    pub(crate) fn is_positive(self) -> bool {
        self.sign == Sign::Positive && !self.is_zero()
    }

    /// Normalizes `-0` into `+0`, leaving non-zero values unchanged.
    pub(crate) fn with_positive_zero_policy(self) -> Self {
        if self.bytes == [0, 0] {
            Self {
                sign: Sign::Positive,
                ..self
            }
        } else {
            self
        }
    }
}
