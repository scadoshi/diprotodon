use crate::{
    domain::{
        cache::{Cache, CacheError},
        command::Command,
    },
    inbound::resp::frame::{Frame, FrameError},
    outbound::resp::reply::{Reply, SimpleInner},
};
use std::io::{BufReader, BufWriter, Read, Write};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReplError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Cache(#[from] CacheError),
}

#[derive(Debug)]
pub struct SessionReader<R: Read> {
    inner: BufReader<R>,
    buf: Vec<u8>,
}

impl<R: Read> SessionReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            inner: BufReader::new(reader),
            buf: Vec::new(),
        }
    }

    pub fn read(&mut self) -> std::io::Result<usize> {
        let mut new = [0u8; 1024];
        let len = self.inner.read(&mut new)?;
        self.buf.extend_from_slice(&new[..len]);
        Ok(len)
    }

    pub fn parse_frame(&mut self) -> Result<Frame, FrameError> {
        match Frame::parse_one(&self.buf) {
            Ok((frame, bytes)) => {
                let consumed = self.buf.len() - bytes.len();
                self.buf.drain(..consumed);
                Ok(frame)
            }
            Err(e) => {
                if !matches!(e, FrameError::Incomplete) {
                    self.buf.clear();
                }
                Err(e)
            }
        }
    }
}

#[derive(Debug)]
pub struct Session<R: Read, W: Write> {
    id: u32,
    reader: SessionReader<R>,
    writer: BufWriter<W>,
    cache: Cache,
}

impl<R: Read, W: Write> Session<R, W> {
    pub fn new(id: u32, reader: R, writer: W, cache: Cache) -> Self {
        let writer = BufWriter::new(writer);
        let reader = SessionReader::new(reader);
        Self {
            id,
            reader,
            writer,
            cache,
        }
    }

    pub fn get_frame(&mut self) -> std::io::Result<Option<Frame>> {
        loop {
            self.reader.read()?;
            match self.reader.parse_frame() {
                Ok(frame) => return Ok(Some(frame)),
                Err(e) => {
                    if !matches!(e, FrameError::Incomplete) {
                        Reply::SimpleError(SimpleInner::sanitized(format!("ERR {}", e)))
                            .write_to(&mut self.writer)?;
                    }
                    self.writer.flush()?;
                    continue;
                }
            }
        }
    }

    pub fn get_command(&mut self) -> std::io::Result<Option<Command>> {
        loop {
            let frame = self.get_frame()?;

            match frame {
                Some(frame) => match Command::try_from(frame) {
                    Ok(cmd) => return Ok(Some(cmd)),
                    Err(e) => {
                        Reply::SimpleError(SimpleInner::sanitized(format!("ERR {}", e)))
                            .write_to(&mut self.writer)?;
                        self.writer.flush()?;
                        continue;
                    }
                },
                None => return Ok(None),
            };
        }
    }

    pub fn execute(&mut self, command: Command) -> Result<(), CacheError> {
        let reply = match command {
            Command::Get { key } => match self.cache.get(&key) {
                Ok(Some(value)) => Reply::BulkString(value),
                Ok(None) => Reply::NullBulk,
                Err(e) => return Err(e),
            },
            Command::Set { key, value } => match self.cache.set(key.as_slice(), value.as_slice()) {
                Ok(_) => Reply::SimpleString(SimpleInner::ok()),
                Err(e) => return Err(e),
            },
            Command::Delete { key } => match self.cache.delete(&key) {
                Ok(Some(_)) => Reply::Integer(1),
                Ok(None) => Reply::Integer(0),
                Err(e) => return Err(e),
            },
            Command::Ping => Reply::SimpleString(SimpleInner::pong()),
        };
        reply.write_to(&mut self.writer)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn repl(&mut self) -> Result<(), ReplError> {
        loop {
            let cmd = self.get_command()?;
            match cmd {
                Some(cmd) => self.execute(cmd)?,
                None => {
                    println!("client {} disconnected", self.id);
                    break;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn session_reader_read() {
        let mut reader = SessionReader::new(Cursor::new(Vec::from(b"foo")));
        assert_eq!(reader.read().unwrap(), 3);
        assert_eq!(reader.read().unwrap(), 0);
    }

    #[test]
    fn session_reader_parse_frame_ok() {
        let mut reader =
            SessionReader::new(Cursor::new(Vec::from(b"$3\r\nfoo\r\n$3\r\nbar\r\nbaz")));
        reader.read().unwrap();
        reader.parse_frame().unwrap();
        assert_eq!(reader.buf.len(), 12);
        reader.parse_frame().unwrap();
        assert_eq!(reader.buf.len(), 3);
    }

    #[test]
    fn session_reader_parse_frame_incomplete_error_retains_buf() {
        let mut reader = SessionReader::new(Cursor::new(Vec::from(b"foo")));
        reader.read().unwrap();
        let _ = reader.parse_frame();
        assert_eq!(reader.buf.len(), 3);
    }

    #[test]
    fn session_reader_parse_frame_non_incomplete_errors_clear_buf() {
        let mut reader = SessionReader::new(Cursor::new(Vec::from(b"foo\r\nbar")));
        reader.read().unwrap();
        let _ = reader.parse_frame();
        assert_eq!(reader.buf.len(), 0);
    }
}
