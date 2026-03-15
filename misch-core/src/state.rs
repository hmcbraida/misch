use crate::instruction::{
    self, AddrTransferMode, AddrTransferTarget, AddressSpec, CompareTarget,
    Instruction, JumpCondition, LoadTarget, OperandSpec, RegisterJumpCondition,
    RegisterJumpTarget, ShiftMode, StoreSource,
};
use crate::io::{CallbackInputDevice, CallbackOutputDevice, DeviceSlot};
use crate::word::{Comparison, MixHalfWord, MixWord, Sign};
use crate::MixError;

const MEMORY_SIZE: usize = 4000;
const DEVICE_COUNT: usize = 21;

/// Complete MIX machine state plus a single-step execution engine.
///
/// `MixState` owns memory, registers, condition flags, I/O device bindings,
/// and the instruction counter. Typical usage is:
///
/// 1. Build a machine with [`MixState::blank`] or via [`crate::assemble`].
/// 2. Load memory and optionally attach device callbacks.
/// 3. Repeatedly call [`MixState::advance_state`] until [`MixState::is_halted`]
///    returns `true`.
///
/// # Examples
///
/// ```
/// use misch_core::assemble;
///
/// let mut machine = assemble("HLT\nEND 0\n").unwrap();
/// while !machine.is_halted() {
///     machine.advance_state().unwrap();
/// }
/// assert!(machine.is_halted());
/// ```
pub struct MixState {
    byte_size: u16,
    memory: [MixWord; MEMORY_SIZE],
    r_a: MixWord,
    r_x: MixWord,
    overflow: bool,
    comparison: Comparison,
    ic: u16,
    r_i: [MixHalfWord; 6],
    r_j: MixHalfWord,
    halted: bool,
    devices: [Option<DeviceSlot>; DEVICE_COUNT],
}

impl MixState {
    /// Creates a blank MIX machine with the requested `byte_size`.
    ///
    /// All registers and memory cells are initialized to `+0`, the instruction
    /// counter starts at `0`, and no I/O devices are attached.
    ///
    /// `byte_size` must be in `64..=100`.
    pub fn blank(byte_size: u16) -> Result<Self, MixError> {
        if !(64..=100).contains(&byte_size) {
            return Err(MixError::InvalidByteSize(byte_size));
        }
        Ok(Self {
            byte_size,
            memory: [MixWord::ZERO; MEMORY_SIZE],
            r_a: MixWord::ZERO,
            r_x: MixWord::ZERO,
            overflow: false,
            comparison: Comparison::Equal,
            ic: 0,
            r_i: [MixHalfWord::ZERO; 6],
            r_j: MixHalfWord::ZERO,
            halted: false,
            devices: std::array::from_fn(|_| None),
        })
    }

    /// Attaches an input callback to a MIX device unit.
    ///
    /// The callback is invoked whenever an `IN` instruction reads from `unit`.
    ///
    /// - `unit`: device number in the MIX range `0..=20`.
    /// - `block_size`: required number of words returned per read.
    /// - `reader`: callback producing signed words for one device read.
    ///
    /// The callback may return fewer or more machine words as `i64`; values are
    /// converted to MIX words using this machine's byte size. During execution,
    /// `IN` enforces that the produced block matches `block_size`.
    pub fn attach_input_callback<F>(
        &mut self,
        unit: u8,
        block_size: usize,
        reader: F,
    ) -> Result<(), MixError>
    where
        F: FnMut() -> Result<Vec<i64>, MixError> + Send + 'static,
    {
        self.ensure_unit(unit)?;
        let byte_size = self.byte_size;
        let mut reader = reader;
        let adapter = CallbackInputDevice::new(block_size, move || {
            let raw = reader()?;
            Ok(raw
                .into_iter()
                .map(|v| MixWord::from_signed(v, byte_size))
                .collect())
        });
        self.devices[usize::from(unit)] =
            Some(DeviceSlot::Input(Box::new(adapter)));
        Ok(())
    }

    /// Attaches an output callback to a MIX device unit.
    ///
    /// The callback is invoked whenever an `OUT` instruction writes to `unit`.
    ///
    /// - `unit`: device number in the MIX range `0..=20`.
    /// - `block_size`: number of words emitted per write.
    /// - `writer`: callback receiving one output block as signed integers.
    ///
    /// MIX words are converted to signed `i64` values before being passed to the
    /// callback.
    pub fn attach_output_callback<F>(
        &mut self,
        unit: u8,
        block_size: usize,
        writer: F,
    ) -> Result<(), MixError>
    where
        F: FnMut(&[i64]) -> Result<(), MixError> + Send + 'static,
    {
        self.ensure_unit(unit)?;
        let byte_size = self.byte_size;
        let mut writer = writer;
        let adapter = CallbackOutputDevice::new(block_size, move |block| {
            let raw: Vec<i64> =
                block.iter().map(|w| w.as_signed_i64(byte_size)).collect();
            writer(&raw)
        });
        self.devices[usize::from(unit)] =
            Some(DeviceSlot::Output(Box::new(adapter)));
        Ok(())
    }

    /// Stores one signed value into memory.
    ///
    /// - `address`: absolute memory address in `0..4000`.
    /// - `value`: signed integer encoded into one MIX word.
    pub fn set_memory_word(
        &mut self,
        address: usize,
        value: i64,
    ) -> Result<(), MixError> {
        if address >= MEMORY_SIZE {
            return Err(MixError::AddressOutOfRange(address as i32));
        }
        self.memory[address] = MixWord::from_signed(value, self.byte_size);
        Ok(())
    }

    pub(crate) fn set_memory_raw(
        &mut self,
        address: usize,
        word: MixWord,
    ) -> Result<(), MixError> {
        if address >= MEMORY_SIZE {
            return Err(MixError::AddressOutOfRange(address as i32));
        }
        word.validate(self.byte_size)?;
        self.memory[address] = word;
        Ok(())
    }

    /// Reads one memory cell as a signed integer.
    ///
    /// `address` must be in `0..4000`.
    pub fn memory_word(&self, address: usize) -> Result<i64, MixError> {
        if address >= MEMORY_SIZE {
            return Err(MixError::AddressOutOfRange(address as i32));
        }
        Ok(self.memory[address].as_signed_i64(self.byte_size))
    }

    /// Encodes and stores a machine instruction word at `address`.
    ///
    /// Parameters map directly to MIX instruction fields:
    ///
    /// - `op_address`: signed address field (`AA`).
    /// - `index`: index register specifier (`I`, where `0` means no indexing).
    /// - `field`: field specification (`F`) as packed `8 * L + R`.
    /// - `opcode`: operation code (`C`).
    pub fn set_instruction(
        &mut self,
        address: usize,
        op_address: i16,
        index: u8,
        field: u8,
        opcode: u8,
    ) -> Result<(), MixError> {
        if index > 6 {
            return Err(MixError::InvalidIndexRegister(index));
        }
        let sign = if op_address < 0 {
            Sign::Negative
        } else {
            Sign::Positive
        };
        let abs = op_address.unsigned_abs();
        let word = MixWord {
            sign,
            bytes: [
                abs / self.byte_size,
                abs % self.byte_size,
                index as u16,
                field as u16,
                opcode as u16,
            ],
        };
        word.validate(self.byte_size)?;
        if address >= MEMORY_SIZE {
            return Err(MixError::AddressOutOfRange(address as i32));
        }
        self.memory[address] = word;
        Ok(())
    }

    /// Executes the instruction at the current instruction counter.
    ///
    /// The instruction counter is incremented before instruction execution,
    /// matching MIX control-flow semantics (so jump instructions can replace it).
    /// Calling this on a halted machine is a no-op.
    pub fn advance_state(&mut self) -> Result<(), MixError> {
        if self.halted {
            return Ok(());
        }

        let instruction_addr =
            self.checked_memory_address(i32::from(self.ic))?;
        let instruction_word = self.memory[instruction_addr];
        let instruction =
            instruction::Instruction::decode(instruction_word, self.byte_size)?;
        self.ic = self.ic.wrapping_add(1);
        self.execute(instruction)
    }

    /// Returns whether `HLT` has been executed.
    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// Returns the current instruction counter (`IC`).
    pub fn instruction_counter(&self) -> u16 {
        self.ic
    }

    pub(crate) fn set_instruction_counter(
        &mut self,
        ic: u16,
    ) -> Result<(), MixError> {
        if usize::from(ic) >= MEMORY_SIZE {
            return Err(MixError::AddressOutOfRange(i32::from(ic)));
        }
        self.ic = ic;
        Ok(())
    }

    /// Returns the overflow toggle state.
    pub fn overflow_flag(&self) -> bool {
        self.overflow
    }

    /// Returns the comparison indicator as `"less"`, `"equal"`, or `"greater"`.
    pub fn comparison_indicator(&self) -> &'static str {
        match self.comparison {
            Comparison::Less => "less",
            Comparison::Equal => "equal",
            Comparison::Greater => "greater",
        }
    }

    /// Returns register `A` as a signed integer.
    pub fn register_a(&self) -> i64 {
        self.r_a.as_signed_i64(self.byte_size)
    }

    /// Returns register `X` as a signed integer.
    pub fn register_x(&self) -> i64 {
        self.r_x.as_signed_i64(self.byte_size)
    }

    /// Returns jump register `J` as a signed integer.
    pub fn register_j(&self) -> i32 {
        self.r_j.as_signed_i32(self.byte_size)
    }

    /// Returns index register `I1..I6` as a signed integer.
    ///
    /// `index` is 1-based and must be in `1..=6`.
    pub fn index_register(&self, index: u8) -> Result<i32, MixError> {
        if !(1..=6).contains(&index) {
            return Err(MixError::InvalidIndexRegister(index));
        }
        Ok(self.r_i[usize::from(index - 1)].as_signed_i32(self.byte_size))
    }

    /// Returns a signed memory window.
    ///
    /// - `start`: first address in the window.
    /// - `length`: number of consecutive words to read.
    ///
    /// Returns an error if the window is outside `0..4000`.
    pub fn memory_window(
        &self,
        start: usize,
        length: usize,
    ) -> Result<Vec<i64>, MixError> {
        if start >= MEMORY_SIZE {
            return Err(MixError::AddressOutOfRange(start as i32));
        }
        let end = start
            .checked_add(length)
            .ok_or(MixError::AddressOutOfRange(i32::MAX))?;
        if end > MEMORY_SIZE {
            return Err(MixError::AddressOutOfRange(end as i32));
        }
        Ok(self.memory[start..end]
            .iter()
            .map(|word| word.as_signed_i64(self.byte_size))
            .collect())
    }

    /// Dispatches a decoded instruction to its concrete executor.
    fn execute(&mut self, instruction: Instruction) -> Result<(), MixError> {
        match instruction {
            Instruction::Nop => Ok(()),
            Instruction::Add(op) => self.exec_add(op),
            Instruction::Sub(op) => self.exec_sub(op),
            Instruction::Mul(op) => self.exec_mul(op),
            Instruction::Div(op) => self.exec_div(op),
            Instruction::Num => self.exec_num(),
            Instruction::Char => self.exec_char(),
            Instruction::Hlt => self.exec_hlt(),
            Instruction::Shift { addr, mode } => self.exec_shift(addr, mode),
            Instruction::Move { addr, count } => self.exec_move(addr, count),
            Instruction::Load {
                target,
                negate,
                operand,
            } => self.exec_load(target, negate, operand),
            Instruction::Store { source, operand } => {
                self.exec_store(source, operand)
            }
            Instruction::Jbus { addr, unit } => self.exec_jbus(addr, unit),
            Instruction::Ioc { addr, unit } => self.exec_ioc(addr, unit),
            Instruction::In { addr, unit } => self.exec_in(addr, unit),
            Instruction::Out { addr, unit } => self.exec_out(addr, unit),
            Instruction::Jred { addr, unit } => self.exec_jred(addr, unit),
            Instruction::Jump { addr, cond } => self.exec_jump(addr, cond),
            Instruction::RegisterJump { addr, target, cond } => {
                self.exec_register_jump(addr, target, cond)
            }
            Instruction::AddrTransfer { addr, target, mode } => {
                self.exec_addr_transfer(addr, target, mode)
            }
            Instruction::Compare { target, operand } => {
                self.exec_compare(target, operand)
            }
        }
    }

    /// Returns `byte_size^5`, the word magnitude modulus.
    fn word_modulus(&self) -> i64 {
        let mut out = 1_i64;
        for _ in 0..5 {
            out *= i64::from(self.byte_size);
        }
        out
    }

    /// Returns `byte_size^2`, the half-word magnitude modulus.
    fn half_modulus(&self) -> i32 {
        (i64::from(self.byte_size) * i64::from(self.byte_size)) as i32
    }

    /// Validates that a device unit is in the supported `0..=20` range.
    fn ensure_unit(&self, unit: u8) -> Result<(), MixError> {
        if usize::from(unit) >= DEVICE_COUNT {
            return Err(MixError::DeviceUnitOutOfRange(unit));
        }
        Ok(())
    }

    /// Computes effective address by applying optional index register offset.
    fn effective_address(&self, addr: AddressSpec) -> Result<i32, MixError> {
        if addr.index > 6 {
            return Err(MixError::InvalidIndexRegister(addr.index));
        }
        let idx_val = if addr.index == 0 {
            0
        } else {
            self.r_i[usize::from(addr.index - 1)].as_signed_i32(self.byte_size)
        };
        Ok(i32::from(addr.address) + idx_val)
    }

    /// Validates and converts a signed address into a memory index.
    fn checked_memory_address(&self, address: i32) -> Result<usize, MixError> {
        if !(0..MEMORY_SIZE as i32).contains(&address) {
            return Err(MixError::AddressOutOfRange(address));
        }
        Ok(address as usize)
    }

    /// Loads and field-slices a word from memory using an operand spec.
    fn word_from_m(&self, op: OperandSpec) -> Result<MixWord, MixError> {
        let m = self.effective_address(op.addr)?;
        let addr = self.checked_memory_address(m)?;
        self.memory[addr].slice(op.field)
    }

    /// Updates instruction counter, optionally storing return address in `J`.
    fn jump_to(
        &mut self,
        destination: i32,
        store_return: bool,
    ) -> Result<(), MixError> {
        let checked = self.checked_memory_address(destination)?;
        if store_return {
            self.r_j =
                MixHalfWord::from_signed(i32::from(self.ic), self.byte_size)
                    .with_positive_zero_policy();
        }
        self.ic = checked as u16;
        Ok(())
    }

    /// Adds signed delta to a full word with MIX overflow/wrap behavior.
    fn add_to_word_with_overflow(
        &mut self,
        current: MixWord,
        delta: i64,
    ) -> MixWord {
        let max_mag = self.word_modulus() - 1;
        let mut sum = current.as_signed_i64(self.byte_size) + delta;
        self.overflow = sum.abs() > max_mag;
        if self.overflow {
            let modulus = self.word_modulus();
            let sign = if sum < 0 {
                Sign::Negative
            } else {
                Sign::Positive
            };
            sum = sum.abs() % modulus;
            if sign == Sign::Negative {
                sum = -sum;
            }
        }
        MixWord::from_signed(sum, self.byte_size)
    }

    /// Adds signed delta to a half-word with MIX overflow/wrap behavior.
    fn add_to_half_with_overflow(
        &mut self,
        current: MixHalfWord,
        delta: i32,
    ) -> MixHalfWord {
        let max_mag = self.half_modulus() - 1;
        let mut sum = current.as_signed_i32(self.byte_size) + delta;
        self.overflow = sum.abs() > max_mag;
        if self.overflow {
            let modulus = self.half_modulus();
            let sign = if sum < 0 {
                Sign::Negative
            } else {
                Sign::Positive
            };
            sum = sum.abs() % modulus;
            if sign == Sign::Negative {
                sum = -sum;
            }
        }
        MixHalfWord::from_signed(sum, self.byte_size)
    }

    /// Implements `ADD`.
    fn exec_add(&mut self, op: OperandSpec) -> Result<(), MixError> {
        let v = self.word_from_m(op)?;
        self.r_a = self.add_to_word_with_overflow(
            self.r_a,
            v.as_signed_i64(self.byte_size),
        );
        Ok(())
    }

    /// Implements `SUB`.
    fn exec_sub(&mut self, op: OperandSpec) -> Result<(), MixError> {
        let v = self.word_from_m(op)?;
        self.r_a = self.add_to_word_with_overflow(
            self.r_a,
            -v.as_signed_i64(self.byte_size),
        );
        Ok(())
    }

    /// Implements `MUL`.
    fn exec_mul(&mut self, op: OperandSpec) -> Result<(), MixError> {
        let v = self.word_from_m(op)?;
        let sign = if self.r_a.sign == v.sign {
            Sign::Positive
        } else {
            Sign::Negative
        };
        let product =
            self.r_a.magnitude(self.byte_size) * v.magnitude(self.byte_size);
        let base = i64::from(self.byte_size);

        let mut digits = [0_u16; 10];
        let mut rem = product;
        for i in (0..10).rev() {
            digits[i] = (rem % base) as u16;
            rem /= base;
        }
        self.r_a = MixWord {
            sign,
            bytes: [digits[0], digits[1], digits[2], digits[3], digits[4]],
        };
        self.r_x = MixWord {
            sign,
            bytes: [digits[5], digits[6], digits[7], digits[8], digits[9]],
        };
        Ok(())
    }

    /// Implements `DIV`.
    fn exec_div(&mut self, op: OperandSpec) -> Result<(), MixError> {
        let v = self.word_from_m(op)?;
        let denom = v.magnitude(self.byte_size);
        if denom == 0 {
            self.overflow = true;
            return Err(MixError::DivisionByZero);
        }

        let base = i64::from(self.byte_size);
        let mut numerator_mag = 0_i64;
        for b in self.r_a.bytes {
            numerator_mag = numerator_mag * base + i64::from(b);
        }
        for b in self.r_x.bytes {
            numerator_mag = numerator_mag * base + i64::from(b);
        }

        if self.r_a.magnitude(self.byte_size) >= denom {
            self.overflow = true;
            return Err(MixError::DivisionOverflow);
        }

        let dividend_sign = self.r_a.sign;
        let quotient_mag = numerator_mag / denom;
        let rem_mag = numerator_mag % denom;

        self.r_a = MixWord::from_signed(
            if dividend_sign == v.sign {
                quotient_mag
            } else {
                -quotient_mag
            },
            self.byte_size,
        );
        self.r_x = MixWord::from_signed(
            if dividend_sign == Sign::Negative {
                -rem_mag
            } else {
                rem_mag
            },
            self.byte_size,
        );
        Ok(())
    }

    /// Implements `NUM`.
    fn exec_num(&mut self) -> Result<(), MixError> {
        let mut number = 0_i64;
        for b in self.r_a.bytes.into_iter().chain(self.r_x.bytes) {
            number = number * 10 + i64::from(b % 10);
        }
        let modulus = self.word_modulus();
        number %= modulus;
        if self.r_a.sign == Sign::Negative {
            number = -number;
        }
        self.r_a = MixWord::from_signed(number, self.byte_size)
            .with_sign(self.r_a.sign);
        Ok(())
    }

    /// Implements `CHAR`.
    fn exec_char(&mut self) -> Result<(), MixError> {
        let mut digits = [0_u16; 10];
        let mut n = self.r_a.magnitude(self.byte_size);
        for i in (0..10).rev() {
            digits[i] = 30 + (n % 10) as u16;
            n /= 10;
        }
        self.r_a.bytes =
            [digits[0], digits[1], digits[2], digits[3], digits[4]];
        self.r_x.bytes =
            [digits[5], digits[6], digits[7], digits[8], digits[9]];
        Ok(())
    }

    /// Implements `HLT`.
    fn exec_hlt(&mut self) -> Result<(), MixError> {
        self.halted = true;
        Ok(())
    }

    /// Implements all `SL*`/`SR*` shift instructions.
    fn exec_shift(
        &mut self,
        addr: AddressSpec,
        mode: ShiftMode,
    ) -> Result<(), MixError> {
        let m = self.effective_address(addr)?;
        let amount = usize::try_from(m.max(0)).unwrap_or(0);
        match mode {
            ShiftMode::Sla => {
                let n = amount.min(5);
                let mut out = [0_u16; 5];
                out[..(5 - n)].copy_from_slice(&self.r_a.bytes[n..]);
                self.r_a.bytes = out;
            }
            ShiftMode::Sra => {
                let n = amount.min(5);
                let mut out = [0_u16; 5];
                out[n..].copy_from_slice(&self.r_a.bytes[..(5 - n)]);
                self.r_a.bytes = out;
            }
            ShiftMode::Slax
            | ShiftMode::Srax
            | ShiftMode::Slc
            | ShiftMode::Src => {
                let mut joined = [0_u16; 10];
                joined[..5].copy_from_slice(&self.r_a.bytes);
                joined[5..].copy_from_slice(&self.r_x.bytes);
                let n = amount % 10;
                match mode {
                    ShiftMode::Slax => {
                        let mut out = [0_u16; 10];
                        out[..(10 - n)].copy_from_slice(&joined[n..]);
                        joined = out;
                    }
                    ShiftMode::Srax => {
                        let mut out = [0_u16; 10];
                        out[n..].copy_from_slice(&joined[..(10 - n)]);
                        joined = out;
                    }
                    ShiftMode::Slc => joined.rotate_left(n),
                    ShiftMode::Src => joined.rotate_right(n),
                    _ => unreachable!(),
                }
                self.r_a.bytes =
                    [joined[0], joined[1], joined[2], joined[3], joined[4]];
                self.r_x.bytes =
                    [joined[5], joined[6], joined[7], joined[8], joined[9]];
            }
            ShiftMode::Slb | ShiftMode::Srb => {
                let bits_per_byte =
                    (f64::from(self.byte_size).log2().ceil()) as u32;
                let total_bits = bits_per_byte * 10;
                let mut value: u128 = 0;
                let base = u128::from(self.byte_size);
                for b in self.r_a.bytes.into_iter().chain(self.r_x.bytes) {
                    value = value * base + u128::from(b);
                }
                let shift = u32::try_from(amount).unwrap_or(u32::MAX);
                let max_mask = if total_bits >= 128 {
                    u128::MAX
                } else {
                    (1_u128 << total_bits) - 1
                };
                value = match mode {
                    ShiftMode::Slb => {
                        value.checked_shl(shift).unwrap_or(0) & max_mask
                    }
                    ShiftMode::Srb => value.checked_shr(shift).unwrap_or(0),
                    _ => unreachable!(),
                };

                let mut digits = [0_u16; 10];
                let mut rem = value;
                for i in (0..10).rev() {
                    digits[i] = (rem % base) as u16;
                    rem /= base;
                }
                self.r_a.bytes =
                    [digits[0], digits[1], digits[2], digits[3], digits[4]];
                self.r_x.bytes =
                    [digits[5], digits[6], digits[7], digits[8], digits[9]];
            }
        }
        Ok(())
    }

    /// Implements `MOVE`.
    fn exec_move(
        &mut self,
        addr: AddressSpec,
        count: u8,
    ) -> Result<(), MixError> {
        let source_start = self.effective_address(addr)?;
        let mut target = self.r_i[0].as_signed_i32(self.byte_size);
        for i in 0..usize::from(count) {
            let src = self.checked_memory_address(source_start + i as i32)?;
            let dst = self.checked_memory_address(target)?;
            self.memory[dst] = self.memory[src];
            target += 1;
        }
        self.r_i[0] = MixHalfWord::from_signed(target, self.byte_size);
        Ok(())
    }

    /// Implements `LD*` and `LD*N` instructions.
    fn exec_load(
        &mut self,
        target: LoadTarget,
        negate: bool,
        op: OperandSpec,
    ) -> Result<(), MixError> {
        let mut v = self.word_from_m(op)?;
        if negate {
            v = v.negate();
        }

        match target {
            LoadTarget::A => self.r_a = v,
            LoadTarget::X => self.r_x = v,
            LoadTarget::I(i) => {
                self.r_i[usize::from(i - 1)] = MixHalfWord::from_word(v)
            }
        }
        Ok(())
    }

    /// Implements `ST*` instructions.
    fn exec_store(
        &mut self,
        source: StoreSource,
        op: OperandSpec,
    ) -> Result<(), MixError> {
        let m = self.effective_address(op.addr)?;
        let addr = self.checked_memory_address(m)?;
        let src = match source {
            StoreSource::A => self.r_a,
            StoreSource::X => self.r_x,
            StoreSource::I(i) => self.r_i[usize::from(i - 1)].to_word(),
            StoreSource::J => self.r_j.to_word(),
            StoreSource::Zero => MixWord::ZERO,
        };
        self.memory[addr].store_field(op.field, src)
    }

    /// Implements `JBUS`.
    fn exec_jbus(
        &mut self,
        addr: AddressSpec,
        unit: u8,
    ) -> Result<(), MixError> {
        self.ensure_unit(unit)?;
        let busy = match self.devices[usize::from(unit)].as_ref() {
            Some(DeviceSlot::Input(d)) => d.busy(),
            Some(DeviceSlot::Output(d)) => d.busy(),
            None => return Err(MixError::DeviceNotAttached(unit)),
        };
        if busy {
            self.jump_to(self.effective_address(addr)?, true)?;
        }
        Ok(())
    }

    /// Implements `IOC`.
    fn exec_ioc(
        &mut self,
        addr: AddressSpec,
        unit: u8,
    ) -> Result<(), MixError> {
        self.ensure_unit(unit)?;
        let command = self.effective_address(addr)?;
        match self.devices[usize::from(unit)].as_mut() {
            Some(DeviceSlot::Input(d)) => d.control(command)?,
            Some(DeviceSlot::Output(d)) => d.control(command)?,
            None => return Err(MixError::DeviceNotAttached(unit)),
        }
        Ok(())
    }

    /// Implements `IN`.
    fn exec_in(&mut self, addr: AddressSpec, unit: u8) -> Result<(), MixError> {
        self.ensure_unit(unit)?;
        let start =
            self.checked_memory_address(self.effective_address(addr)?)?;
        let block = match self.devices[usize::from(unit)].as_mut() {
            Some(DeviceSlot::Input(d)) => {
                let data = d.read_block()?;
                if data.len() != d.block_size() {
                    return Err(MixError::DeviceBlockSizeMismatch {
                        unit,
                        expected: d.block_size(),
                        got: data.len(),
                    });
                }
                data
            }
            Some(DeviceSlot::Output(_)) => {
                return Err(MixError::WrongDeviceDirection {
                    unit,
                    expected: "input",
                });
            }
            None => return Err(MixError::DeviceNotAttached(unit)),
        };

        for (offset, word) in block.into_iter().enumerate() {
            word.validate(self.byte_size)?;
            let dst =
                self.checked_memory_address(start as i32 + offset as i32)?;
            self.memory[dst] = word;
        }
        Ok(())
    }

    /// Implements `OUT`.
    fn exec_out(
        &mut self,
        addr: AddressSpec,
        unit: u8,
    ) -> Result<(), MixError> {
        self.ensure_unit(unit)?;
        let start =
            self.checked_memory_address(self.effective_address(addr)?)?;

        let count = match self.devices[usize::from(unit)].as_ref() {
            Some(DeviceSlot::Output(d)) => d.block_size(),
            Some(DeviceSlot::Input(_)) => {
                return Err(MixError::WrongDeviceDirection {
                    unit,
                    expected: "output",
                });
            }
            None => return Err(MixError::DeviceNotAttached(unit)),
        };

        let mut block = Vec::with_capacity(count);
        for i in 0..count {
            let src = self.checked_memory_address(start as i32 + i as i32)?;
            block.push(self.memory[src]);
        }

        match self.devices[usize::from(unit)].as_mut() {
            Some(DeviceSlot::Output(d)) => d.write_block(&block),
            Some(DeviceSlot::Input(_)) => Err(MixError::WrongDeviceDirection {
                unit,
                expected: "output",
            }),
            None => Err(MixError::DeviceNotAttached(unit)),
        }
    }

    /// Implements `JRED`.
    fn exec_jred(
        &mut self,
        addr: AddressSpec,
        unit: u8,
    ) -> Result<(), MixError> {
        self.ensure_unit(unit)?;
        let busy = match self.devices[usize::from(unit)].as_ref() {
            Some(DeviceSlot::Input(d)) => d.busy(),
            Some(DeviceSlot::Output(d)) => d.busy(),
            None => return Err(MixError::DeviceNotAttached(unit)),
        };
        if !busy {
            self.jump_to(self.effective_address(addr)?, true)?;
        }
        Ok(())
    }

    /// Implements opcode 39 jump family (`JMP`/`JSJ`/`JOV`/...).
    fn exec_jump(
        &mut self,
        addr: AddressSpec,
        cond: JumpCondition,
    ) -> Result<(), MixError> {
        let should_jump = match cond {
            JumpCondition::Jmp | JumpCondition::Jsj => true,
            JumpCondition::Jov => {
                let v = self.overflow;
                self.overflow = false;
                v
            }
            JumpCondition::Jnov => {
                let v = !self.overflow;
                self.overflow = false;
                v
            }
            JumpCondition::Jl => self.comparison == Comparison::Less,
            JumpCondition::Je => self.comparison == Comparison::Equal,
            JumpCondition::Jg => self.comparison == Comparison::Greater,
            JumpCondition::Jge => self.comparison != Comparison::Less,
            JumpCondition::Jne => self.comparison != Comparison::Equal,
            JumpCondition::Jle => self.comparison != Comparison::Greater,
        };
        if should_jump {
            self.jump_to(
                self.effective_address(addr)?,
                !matches!(cond, JumpCondition::Jsj),
            )?;
        }
        Ok(())
    }

    /// Implements `JA*`/`JX*`/`Ji*` families.
    fn exec_register_jump(
        &mut self,
        addr: AddressSpec,
        target: RegisterJumpTarget,
        cond: RegisterJumpCondition,
    ) -> Result<(), MixError> {
        let should_jump = match target {
            RegisterJumpTarget::A => Self::jump_predicate_word(self.r_a, cond),
            RegisterJumpTarget::X => Self::jump_predicate_word(self.r_x, cond),
            RegisterJumpTarget::I(i) => {
                Self::jump_predicate_half(self.r_i[usize::from(i - 1)], cond)
            }
        };

        if should_jump {
            self.jump_to(self.effective_address(addr)?, true)?;
        }
        Ok(())
    }

    /// Evaluates register-jump predicate for full-word registers (`A`/`X`).
    fn jump_predicate_word(
        value: MixWord,
        cond: RegisterJumpCondition,
    ) -> bool {
        let is_zero = value.bytes == [0; 5];
        // MIX has both +0 and -0. We treat -0 as negative for sign-based jumps.
        let is_neg = value.sign == Sign::Negative;
        let is_pos = value.sign == Sign::Positive && !is_zero;
        let odd = value.bytes[4] % 2 == 1;
        match cond {
            RegisterJumpCondition::Negative => is_neg,
            RegisterJumpCondition::Zero => is_zero,
            RegisterJumpCondition::Positive => is_pos,
            RegisterJumpCondition::NonNegative => !is_neg,
            RegisterJumpCondition::NonZero => !is_zero,
            RegisterJumpCondition::NonPositive => !is_pos,
            RegisterJumpCondition::Even => !odd,
            RegisterJumpCondition::Odd => odd,
        }
    }

    /// Evaluates register-jump predicate for half-word index registers.
    fn jump_predicate_half(
        value: MixHalfWord,
        cond: RegisterJumpCondition,
    ) -> bool {
        let is_zero = value.is_zero();
        let is_neg = value.is_negative();
        let is_pos = value.is_positive();
        match cond {
            RegisterJumpCondition::Negative => is_neg,
            RegisterJumpCondition::Zero => is_zero,
            RegisterJumpCondition::Positive => is_pos,
            RegisterJumpCondition::NonNegative => !is_neg,
            RegisterJumpCondition::NonZero => !is_zero,
            RegisterJumpCondition::NonPositive => !is_pos,
            RegisterJumpCondition::Even => true,
            RegisterJumpCondition::Odd => false,
        }
    }

    /// Implements `ENT*`/`ENN*`/`INC*`/`DEC*` families.
    fn exec_addr_transfer(
        &mut self,
        addr: AddressSpec,
        target: AddrTransferTarget,
        mode: AddrTransferMode,
    ) -> Result<(), MixError> {
        let m = self.effective_address(addr)?;
        match target {
            AddrTransferTarget::A => match mode {
                AddrTransferMode::Ent => {
                    self.r_a =
                        MixWord::from_signed(i64::from(m), self.byte_size)
                }
                AddrTransferMode::Enn => {
                    self.r_a =
                        MixWord::from_signed(i64::from(-m), self.byte_size)
                }
                AddrTransferMode::Inc => {
                    self.r_a =
                        self.add_to_word_with_overflow(self.r_a, i64::from(m));
                }
                AddrTransferMode::Dec => {
                    self.r_a =
                        self.add_to_word_with_overflow(self.r_a, i64::from(-m));
                }
            },
            AddrTransferTarget::X => match mode {
                AddrTransferMode::Ent => {
                    self.r_x =
                        MixWord::from_signed(i64::from(m), self.byte_size)
                }
                AddrTransferMode::Enn => {
                    self.r_x =
                        MixWord::from_signed(i64::from(-m), self.byte_size)
                }
                AddrTransferMode::Inc => {
                    self.r_x =
                        self.add_to_word_with_overflow(self.r_x, i64::from(m));
                }
                AddrTransferMode::Dec => {
                    self.r_x =
                        self.add_to_word_with_overflow(self.r_x, i64::from(-m));
                }
            },
            AddrTransferTarget::I(i) => {
                let idx = usize::from(i - 1);
                match mode {
                    AddrTransferMode::Ent => {
                        self.r_i[idx] =
                            MixHalfWord::from_signed(m, self.byte_size)
                    }
                    AddrTransferMode::Enn => {
                        self.r_i[idx] =
                            MixHalfWord::from_signed(-m, self.byte_size)
                    }
                    AddrTransferMode::Inc => {
                        self.r_i[idx] =
                            self.add_to_half_with_overflow(self.r_i[idx], m)
                    }
                    AddrTransferMode::Dec => {
                        self.r_i[idx] =
                            self.add_to_half_with_overflow(self.r_i[idx], -m)
                    }
                }
            }
        }
        Ok(())
    }

    /// Implements `CMP*` family.
    fn exec_compare(
        &mut self,
        target: CompareTarget,
        op: OperandSpec,
    ) -> Result<(), MixError> {
        let v = self.word_from_m(op)?;
        self.comparison = match target {
            CompareTarget::A => Self::cmp_word(self.r_a, v, self.byte_size),
            CompareTarget::X => Self::cmp_word(self.r_x, v, self.byte_size),
            CompareTarget::I(i) => {
                let left = self.r_i[usize::from(i - 1)];
                let right = MixHalfWord::from_word(v);
                Self::cmp_half(left, right, self.byte_size)
            }
        };
        Ok(())
    }

    /// Compares two words as signed values.
    fn cmp_word(lhs: MixWord, rhs: MixWord, byte_size: u16) -> Comparison {
        let l = lhs.as_signed_i64(byte_size);
        let r = rhs.as_signed_i64(byte_size);
        if l < r {
            Comparison::Less
        } else if l > r {
            Comparison::Greater
        } else {
            Comparison::Equal
        }
    }

    /// Compares two half-words as signed values.
    fn cmp_half(
        lhs: MixHalfWord,
        rhs: MixHalfWord,
        byte_size: u16,
    ) -> Comparison {
        let l = lhs.as_signed_i32(byte_size);
        let r = rhs.as_signed_i32(byte_size);
        if l < r {
            Comparison::Less
        } else if l > r {
            Comparison::Greater
        } else {
            Comparison::Equal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    const BYTE_SIZE: u16 = 64;

    fn machine() -> MixState {
        MixState::blank(BYTE_SIZE).unwrap()
    }

    fn address(address: i16, index: u8) -> AddressSpec {
        AddressSpec { address, index }
    }

    fn operand(op_address: i16, index: u8, field: u8) -> OperandSpec {
        OperandSpec {
            addr: address(op_address, index),
            field,
        }
    }

    fn instr(instruction: Instruction) -> MixWord {
        instruction.encode(BYTE_SIZE)
    }

    #[test]
    fn load_and_store_field_behaviour() {
        let mut s = machine();
        s.memory[10] = MixWord {
            sign: Sign::Negative,
            bytes: [1, 2, 3, 4, 5],
        };
        s.memory[0] = instr(Instruction::Load {
            target: LoadTarget::A,
            negate: false,
            operand: operand(10, 0, 3 * 8 + 4),
        });
        s.advance_state().unwrap();
        assert_eq!(s.r_a.sign, Sign::Positive);
        assert_eq!(s.r_a.bytes, [0, 0, 0, 3, 4]);

        s.memory[1] = instr(Instruction::Store {
            source: StoreSource::A,
            operand: operand(11, 0, 2 * 8 + 3),
        });
        s.memory[11] = MixWord {
            sign: Sign::Negative,
            bytes: [9, 9, 9, 9, 9],
        };
        s.ic = 1;
        s.advance_state().unwrap();
        assert_eq!(s.memory[11].sign, Sign::Negative);
        assert_eq!(s.memory[11].bytes, [9, 3, 4, 9, 9]);
    }

    #[test]
    fn add_sets_overflow_and_wraps() {
        let mut s = machine();
        s.r_a = MixWord::from_signed(s.word_modulus() - 1, s.byte_size);
        s.memory[100] = MixWord::from_signed(2, s.byte_size);
        s.memory[0] = instr(Instruction::Add(operand(100, 0, 5)));
        s.advance_state().unwrap();
        assert!(s.overflow);
        assert_eq!(s.r_a.as_signed_i64(s.byte_size), 1);
    }

    #[test]
    fn sub_and_cmp_work() {
        let mut s = machine();
        s.r_a = MixWord::from_signed(20, BYTE_SIZE);
        s.memory[20] = MixWord::from_signed(7, 64);
        s.memory[0] = instr(Instruction::Sub(operand(20, 0, 5)));
        s.memory[1] = instr(Instruction::Compare {
            target: CompareTarget::A,
            operand: operand(20, 0, 5),
        });
        s.advance_state().unwrap();
        s.advance_state().unwrap();
        assert_eq!(s.r_a.as_signed_i64(64), 13);
        assert_eq!(s.comparison, Comparison::Greater);
    }

    #[test]
    fn mul_and_div() {
        let mut s = machine();
        s.r_a = MixWord::from_signed(1234, 64);
        s.memory[10] = MixWord::from_signed(12, 64);
        s.memory[0] = instr(Instruction::Mul(operand(10, 0, 5)));
        s.advance_state().unwrap();
        assert_eq!(
            s.r_a.magnitude(64) * s.word_modulus() + s.r_x.magnitude(64),
            14808
        );

        s.memory[1] = instr(Instruction::Div(operand(10, 0, 5)));
        s.ic = 1;
        s.advance_state().unwrap();
        assert_eq!(s.r_a.as_signed_i64(64), 1234);
        assert_eq!(s.r_x.as_signed_i64(64), 0);
    }

    #[test]
    fn num_char_hlt() {
        let mut s = machine();
        s.r_a.bytes = [30, 31, 32, 33, 34];
        s.r_x.bytes = [35, 36, 37, 38, 39];
        s.memory[0] = instr(Instruction::Num);
        s.advance_state().unwrap();
        assert_eq!(s.r_a.as_signed_i64(64), 123456789 % s.word_modulus());

        s.memory[1] = instr(Instruction::Char);
        s.ic = 1;
        s.advance_state().unwrap();
        assert!(s.r_a.bytes.iter().all(|b| (30..=39).contains(b)));
        assert!(s.r_x.bytes.iter().all(|b| (30..=39).contains(b)));

        s.memory[2] = instr(Instruction::Hlt);
        s.ic = 2;
        s.advance_state().unwrap();
        assert!(s.is_halted());
    }

    #[test]
    fn shifts_cover_byte_and_circular_modes() {
        let mut s = machine();
        s.r_a.bytes = [1, 2, 3, 4, 5];
        s.r_x.bytes = [6, 7, 8, 9, 10];
        s.memory[0] = instr(Instruction::Shift {
            addr: address(2, 0),
            mode: ShiftMode::Sla,
        });
        s.advance_state().unwrap();
        assert_eq!(s.r_a.bytes, [3, 4, 5, 0, 0]);

        s.memory[1] = instr(Instruction::Shift {
            addr: address(3, 0),
            mode: ShiftMode::Slc,
        });
        s.ic = 1;
        s.advance_state().unwrap();
        assert_eq!(s.r_a.bytes, [0, 0, 6, 7, 8]);
        assert_eq!(s.r_x.bytes, [9, 10, 3, 4, 5]);
    }

    #[test]
    fn bit_shifts_do_not_panic_for_very_large_amounts() {
        let mut s = machine();
        s.r_a.bytes = [1, 2, 3, 4, 5];
        s.r_x.bytes = [6, 7, 8, 9, 10];
        s.r_i[0] = MixHalfWord::from_signed(4095, BYTE_SIZE);

        s.memory[0] = instr(Instruction::Shift {
            addr: address(4095, 1),
            mode: ShiftMode::Slb,
        });
        s.memory[1] = instr(Instruction::Shift {
            addr: address(4095, 1),
            mode: ShiftMode::Srb,
        });

        s.advance_state().unwrap();
        assert_eq!(s.r_a.bytes, [0, 0, 0, 0, 0]);
        assert_eq!(s.r_x.bytes, [0, 0, 0, 0, 0]);

        s.advance_state().unwrap();
        assert_eq!(s.r_a.bytes, [0, 0, 0, 0, 0]);
        assert_eq!(s.r_x.bytes, [0, 0, 0, 0, 0]);
    }

    #[test]
    fn move_moves_and_updates_i1() {
        let mut s = machine();
        s.memory[200] = MixWord::from_signed(1, 64);
        s.memory[201] = MixWord::from_signed(2, 64);
        s.r_i[0] = MixHalfWord::from_signed(300, 64);
        s.memory[0] = instr(Instruction::Move {
            addr: address(200, 0),
            count: 2,
        });
        s.advance_state().unwrap();
        assert_eq!(s.memory[300].as_signed_i64(64), 1);
        assert_eq!(s.memory[301].as_signed_i64(64), 2);
        assert_eq!(s.r_i[0].as_signed_i32(64), 302);
    }

    #[test]
    fn jump_family_and_overflow_reset() {
        let mut s = machine();
        s.overflow = true;
        s.memory[0] = instr(Instruction::Jump {
            addr: address(100, 0),
            cond: JumpCondition::Jov,
        });
        s.advance_state().unwrap();
        assert_eq!(s.ic, 100);
        assert!(!s.overflow);
        assert_eq!(s.r_j.as_signed_i32(64), 1);

        s.memory[100] = instr(Instruction::Jump {
            addr: address(200, 0),
            cond: JumpCondition::Jsj,
        });
        s.advance_state().unwrap();
        assert_eq!(s.ic, 200);
        assert_eq!(s.r_j.as_signed_i32(64), 1);
    }

    #[test]
    fn program_can_self_modify_instruction_memory() {
        let mut s = machine();

        // Preload register A with the encoded HLT instruction so we can write it
        // directly into instruction memory.
        s.r_a = instr(Instruction::Hlt);

        // Program layout:
        //   0: STA 2   ; overwrite instruction at address 2 with A
        //   1: JSJ 2   ; jump to the modified instruction
        //   2: NOP     ; will be replaced by HLT at runtime
        s.memory[0] = instr(Instruction::Store {
            source: StoreSource::A,
            operand: operand(2, 0, 5),
        });
        s.memory[1] = instr(Instruction::Jump {
            addr: address(2, 0),
            cond: JumpCondition::Jsj,
        });
        s.memory[2] = instr(Instruction::Nop);

        // Step 1: mutate code memory.
        s.advance_state().unwrap();
        assert_eq!(s.memory[2].bytes, instr(Instruction::Hlt).bytes);
        assert_eq!(s.memory[2].sign, instr(Instruction::Hlt).sign);

        // Step 2: branch to the patched location.
        s.advance_state().unwrap();
        assert_eq!(s.ic, 2);
        assert!(!s.is_halted());

        // Step 3: execute the patched instruction and halt.
        s.advance_state().unwrap();
        assert!(s.is_halted());
    }

    #[test]
    fn register_jump_and_negative_zero_policy() {
        let mut s = machine();
        s.r_a = MixWord {
            sign: Sign::Negative,
            bytes: [0; 5],
        };
        s.memory[0] = instr(Instruction::RegisterJump {
            addr: address(123, 0),
            target: RegisterJumpTarget::A,
            cond: RegisterJumpCondition::Negative,
        });
        s.advance_state().unwrap();
        assert_eq!(s.ic, 123);
    }

    #[test]
    fn address_transfer_variants() {
        let mut s = machine();
        s.memory[0] = instr(Instruction::AddrTransfer {
            addr: address(10, 0),
            target: AddrTransferTarget::A,
            mode: AddrTransferMode::Ent,
        });
        s.memory[1] = instr(Instruction::AddrTransfer {
            addr: address(5, 0),
            target: AddrTransferTarget::A,
            mode: AddrTransferMode::Inc,
        });
        s.memory[2] = instr(Instruction::AddrTransfer {
            addr: address(7, 0),
            target: AddrTransferTarget::X,
            mode: AddrTransferMode::Enn,
        });
        s.advance_state().unwrap();
        assert_eq!(s.r_a.as_signed_i64(64), 10);
        s.advance_state().unwrap();
        assert_eq!(s.r_a.as_signed_i64(64), 15);
        s.advance_state().unwrap();
        assert_eq!(s.r_x.as_signed_i64(64), -7);
    }

    #[test]
    fn io_in_out_and_missing_device_error() {
        let mut s = machine();
        let input_words =
            [MixWord::from_signed(9, 64), MixWord::from_signed(8, 64)];
        s.attach_input_callback(16, 2, move || {
            Ok(input_words
                .iter()
                .map(|w| w.as_signed_i64(64))
                .collect::<Vec<_>>())
        })
        .unwrap();

        let captured: Arc<Mutex<Vec<i64>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_out = Arc::clone(&captured);
        s.attach_output_callback(17, 2, move |block| {
            captured_out.lock().unwrap().extend_from_slice(block);
            Ok(())
        })
        .unwrap();

        s.memory[0] = instr(Instruction::In {
            addr: address(500, 0),
            unit: 16,
        });
        s.memory[1] = instr(Instruction::Out {
            addr: address(500, 0),
            unit: 17,
        });
        s.advance_state().unwrap();
        assert_eq!(s.memory[500].as_signed_i64(64), 9);
        assert_eq!(s.memory[501].as_signed_i64(64), 8);

        s.advance_state().unwrap();
        assert_eq!(captured.lock().unwrap().len(), 2);

        s.memory[2] = instr(Instruction::In {
            addr: address(0, 0),
            unit: 5,
        });
        s.ic = 2;
        let err = s.advance_state().unwrap_err();
        assert!(matches!(err, MixError::DeviceNotAttached(5)));
    }

    #[test]
    fn jbus_jred_use_device_busy_state() {
        let mut s = machine();
        s.attach_input_callback(0, 1, || Ok(vec![0])).unwrap();
        s.memory[0] = instr(Instruction::Jbus {
            addr: address(200, 0),
            unit: 0,
        });
        s.memory[1] = instr(Instruction::Jred {
            addr: address(300, 0),
            unit: 0,
        });
        s.advance_state().unwrap();
        assert_eq!(s.ic, 1);
        s.advance_state().unwrap();
        assert_eq!(s.ic, 300);
    }

    #[test]
    fn advance_state_errors_when_instruction_counter_out_of_bounds() {
        let mut s = machine();
        s.ic = MEMORY_SIZE as u16;

        let err = s.advance_state().unwrap_err();
        assert!(matches!(
            err,
            MixError::AddressOutOfRange(addr) if addr == MEMORY_SIZE as i32
        ));
    }
}
