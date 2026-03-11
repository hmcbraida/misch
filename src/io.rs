use crate::error::MixError;
use crate::word::MixWord;

pub(crate) trait InputDevice {
    fn block_size(&self) -> usize;
    fn read_block(&mut self) -> Result<Vec<MixWord>, MixError>;
    fn control(&mut self, _command: i32) -> Result<(), MixError> {
        Ok(())
    }
    fn busy(&self) -> bool {
        false
    }
}

pub(crate) trait OutputDevice {
    fn block_size(&self) -> usize;
    fn write_block(&mut self, block: &[MixWord]) -> Result<(), MixError>;
    fn control(&mut self, _command: i32) -> Result<(), MixError> {
        Ok(())
    }
    fn busy(&self) -> bool {
        false
    }
}

pub(crate) struct CallbackInputDevice {
    block: usize,
    reader: Box<dyn FnMut() -> Result<Vec<MixWord>, MixError>>,
}

impl CallbackInputDevice {
    pub(crate) fn new<F>(block: usize, reader: F) -> Self
    where
        F: FnMut() -> Result<Vec<MixWord>, MixError> + 'static,
    {
        Self {
            block,
            reader: Box::new(reader),
        }
    }
}

impl InputDevice for CallbackInputDevice {
    fn block_size(&self) -> usize {
        self.block
    }

    fn read_block(&mut self) -> Result<Vec<MixWord>, MixError> {
        (self.reader)()
    }
}

pub(crate) struct CallbackOutputDevice {
    block: usize,
    writer: Box<dyn FnMut(&[MixWord]) -> Result<(), MixError>>,
}

impl CallbackOutputDevice {
    pub(crate) fn new<F>(block: usize, writer: F) -> Self
    where
        F: FnMut(&[MixWord]) -> Result<(), MixError> + 'static,
    {
        Self {
            block,
            writer: Box::new(writer),
        }
    }
}

impl OutputDevice for CallbackOutputDevice {
    fn block_size(&self) -> usize {
        self.block
    }

    fn write_block(&mut self, block: &[MixWord]) -> Result<(), MixError> {
        (self.writer)(block)
    }
}

pub(crate) enum DeviceSlot {
    Input(Box<dyn InputDevice>),
    Output(Box<dyn OutputDevice>),
}
