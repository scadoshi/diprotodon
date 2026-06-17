pub mod read;
pub mod write;

use read::ReadCommand;
use write::WriteCommand;

#[derive(Debug, Clone, PartialEq)]
pub enum CacheCommand {
    Read(ReadCommand),
    Write(WriteCommand),
}

impl From<ReadCommand> for CacheCommand {
    fn from(value: ReadCommand) -> Self {
        Self::Read(value)
    }
}

impl From<WriteCommand> for CacheCommand {
    fn from(value: WriteCommand) -> Self {
        Self::Write(value)
    }
}
