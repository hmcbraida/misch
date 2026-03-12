use crate::error::MixError;
use crate::word::MixWord;

type InputCallback = dyn FnMut() -> Result<Vec<MixWord>, MixError> + Send;
type OutputCallback = dyn FnMut(&[MixWord]) -> Result<(), MixError> + Send;

/// Input-side device contract used by MIX `IN`, `JBUS`, and `JRED`.
pub(crate) trait InputDevice: Send {
    /// Returns the configured transfer block size in words.
    fn block_size(&self) -> usize;
    /// Reads and returns one input block.
    fn read_block(&mut self) -> Result<Vec<MixWord>, MixError>;
    /// Applies a device-specific control command (`IOC`).
    fn control(&mut self, _command: i32) -> Result<(), MixError> {
        Ok(())
    }
    /// Returns whether the device is currently busy.
    fn busy(&self) -> bool {
        false
    }
}

/// Output-side device contract used by MIX `OUT`, `JBUS`, and `JRED`.
pub(crate) trait OutputDevice: Send {
    /// Returns the configured transfer block size in words.
    fn block_size(&self) -> usize;
    /// Writes one output block.
    fn write_block(&mut self, block: &[MixWord]) -> Result<(), MixError>;
    /// Applies a device-specific control command (`IOC`).
    fn control(&mut self, _command: i32) -> Result<(), MixError> {
        Ok(())
    }
    /// Returns whether the device is currently busy.
    fn busy(&self) -> bool {
        false
    }
}

/// Input device backed by a Rust callback.
pub(crate) struct CallbackInputDevice {
    block: usize,
    reader: Box<InputCallback>,
}

impl CallbackInputDevice {
    /// Creates a callback-backed input device.
    pub(crate) fn new<F>(block: usize, reader: F) -> Self
    where
        F: FnMut() -> Result<Vec<MixWord>, MixError> + Send + 'static,
    {
        Self {
            block,
            reader: Box::new(reader),
        }
    }
}

impl InputDevice for CallbackInputDevice {
    /// Returns configured callback block size.
    fn block_size(&self) -> usize {
        self.block
    }

    /// Delegates one read request to the stored callback.
    fn read_block(&mut self) -> Result<Vec<MixWord>, MixError> {
        (self.reader)()
    }
}

/// Output device backed by a Rust callback.
pub(crate) struct CallbackOutputDevice {
    block: usize,
    writer: Box<OutputCallback>,
}

impl CallbackOutputDevice {
    /// Creates a callback-backed output device.
    pub(crate) fn new<F>(block: usize, writer: F) -> Self
    where
        F: FnMut(&[MixWord]) -> Result<(), MixError> + Send + 'static,
    {
        Self {
            block,
            writer: Box::new(writer),
        }
    }
}

impl OutputDevice for CallbackOutputDevice {
    /// Returns configured callback block size.
    fn block_size(&self) -> usize {
        self.block
    }

    /// Delegates one write request to the stored callback.
    fn write_block(&mut self, block: &[MixWord]) -> Result<(), MixError> {
        (self.writer)(block)
    }
}

/// Attached device in a unit slot.
pub(crate) enum DeviceSlot {
    /// Input-only device.
    Input(Box<dyn InputDevice + Send>),
    /// Output-only device.
    Output(Box<dyn OutputDevice + Send>),
}
