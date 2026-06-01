//! Per-connection session — the REPL that drives a single TCP client.
//!
//! [`Session`] owns the buffered reader/writer for one connection and the shared
//! [`Cache`] handle. Its loop is: read bytes → parse a frame → lift it to a [`Command`]
//! → execute against the cache → write a [`Reply`] back. Malformed frames and unknown
//! commands produce a `-ERR ...\r\n` reply but do not kill the session.
//!
//! Generic over `R: Read` / `W: Write` so tests can drive a session with a
//! `Cursor<Vec<u8>>` reader and a `Vec<u8>` writer — no actual TCP needed.

use crate::{
    domain::{
        cache::{Cache, CacheError, Entry},
        command::Command,
    },
    inbound::resp::frame::{Frame, FrameError},
    outbound::resp::reply::{Reply, SimpleInner},
};
use std::{
    io::{BufReader, BufWriter, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use thiserror::Error;

/// Fatal errors from the REPL loop. Non-fatal errors (bad frames, unknown commands)
/// are converted to RESP error replies inside the loop and do not surface here.
#[derive(Debug, Error)]
pub enum ReplError {
    /// I/O failed on the socket — the client probably went away.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Cache layer error during command execution.
    #[error(transparent)]
    Cache(#[from] CacheError),
}

/// Buffered reader for one connection's incoming byte stream.
///
/// Owns an accumulation buffer that survives across `read` calls. The frame parser
/// drains the prefix it consumes; an `Incomplete` parse preserves the buffer so the
/// next `read` extends it and parsing can retry with more bytes.
#[derive(Debug)]
pub struct SessionReader<R: Read> {
    inner: BufReader<R>,
    buf: Vec<u8>,
}

impl<R: Read> SessionReader<R> {
    /// Wrap a reader. The buffer starts empty; bytes accumulate as `read` is called.
    pub fn new(reader: R) -> Self {
        Self {
            inner: BufReader::new(reader),
            buf: Vec::new(),
        }
    }

    /// Pull up to 1024 bytes off the underlying reader and append them to the
    /// accumulation buffer. Returns the number of bytes appended (0 on EOF).
    pub fn read(&mut self) -> std::io::Result<usize> {
        let mut new = [0u8; 1024];
        let len = self.inner.read(&mut new)?;
        self.buf.extend_from_slice(&new[..len]);
        Ok(len)
    }

    /// Try to parse one frame off the front of the buffer. Three outcomes:
    ///
    /// - **Ok** — parsed; the consumed prefix is drained from the buffer.
    /// - **Err(Incomplete)** — buffer preserved unchanged so callers can `read` more
    ///   bytes and retry.
    /// - **Err(other)** — buffer is cleared. A hard parse error means the byte stream
    ///   is no longer aligned with the protocol; resync by dropping everything and
    ///   starting fresh on the next read.
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

/// One client's REPL state. Generic so it can be driven over real TCP streams or
/// in-memory test buffers without changing the loop.
#[derive(Debug)]
pub struct Session<R: Read, W: Write> {
    id: u32,
    reader: SessionReader<R>,
    writer: BufWriter<W>,
    cache: Cache,
}

impl<R: Read, W: Write> Session<R, W> {
    /// Build a session for a single connection. `id` is just a numeric tag used in
    /// connection-lifecycle logs; the cache is shared across all sessions via its
    /// internal `Arc`.
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

    /// Read until exactly one frame can be parsed off the buffer.
    ///
    /// On `Incomplete`, reads more bytes and retries. On any other parse error, writes
    /// a `-ERR ...\r\n` to the client and retries — the session stays alive after a
    /// malformed frame. Returns `Ok(None)` only if the underlying reader ever yields
    /// `Ok(0)` (clean disconnect).
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

    /// Drive `get_frame` until a frame parses cleanly *and* lifts into a valid
    /// [`Command`]. If a frame parses but is the wrong shape for a command (bad arity,
    /// unknown verb, etc.), writes a `-ERR ...\r\n` and pulls the next frame. Returns
    /// `Ok(None)` only when the underlying reader hits EOF.
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

    /// Dispatch a single [`Command`] against the cache and write the resulting
    /// [`Reply`] to the connection. This is where wire semantics for each command
    /// live — most notably the TTL reply ladder (`:-2` / `:-1` / `:n`) and the
    /// boolean-as-integer convention used by DEL / EXISTS / EXPIRE / PERSIST.
    pub fn execute(&mut self, command: Command) -> Result<(), CacheError> {
        let reply = match command {
            Command::Get { key } => {
                let value = self.cache.get(&key)?;
                match value {
                    Some(Entry { value, .. }) => Reply::BulkString(value),
                    None => Reply::NullBulk,
                }
            }
            Command::Set { key, value } => {
                self.cache
                    .insert(key.as_slice(), Entry::new(value.as_slice(), None))?;
                Reply::SimpleString(SimpleInner::ok())
            }
            Command::Delete { key } => Reply::Integer(self.cache.remove(&key)?.is_some() as i64),
            Command::Ping {
                message: Some(message),
            } => Reply::BulkString(message),
            Command::Ping { message: None } => Reply::SimpleString(SimpleInner::pong()),
            Command::Exists { key } => Reply::Integer(self.cache.contains(&key)? as i64),
            Command::Expire { key, relative_ttl } => {
                Reply::Integer(self.cache.set_relative_ttl(&key, relative_ttl)? as i64)
            }
            Command::ExpireAt { key, absolute_ttl } => {
                Reply::Integer(self.cache.set_absolute_ttl(&key, absolute_ttl)? as i64)
            }
            Command::TTL { key } => Reply::Integer(match self.cache.get_relative_ttl(&key)? {
                None => -2,
                Some(None) => -1,
                Some(Some(ttl)) => i64::try_from(ttl).unwrap_or(i64::MAX),
            }),
            Command::Persist { key } => Reply::Integer(self.cache.remove_ttl(&key)? as i64),
        };
        reply.write_to(&mut self.writer)?;
        self.writer.flush()?;
        Ok(())
    }

    /// The REPL — read commands and execute them until the client disconnects or the
    /// shared `shutdown_signal` flips.
    ///
    /// The shutdown check fires *between* commands, so an in-flight command always
    /// finishes cleanly. A client parked inside an idle `read` won't notice the flag
    /// until bytes arrive (or until the underlying stream gets a read timeout — see
    /// the "Connection lifecycle" section of `context/plan.md`).
    pub fn repl(&mut self, shutdown_signal: Arc<AtomicBool>) -> Result<(), ReplError> {
        loop {
            let cmd = self.get_command()?;
            match cmd {
                Some(cmd) => self.execute(cmd)?,
                None => {
                    println!("client {} disconnected", self.id);
                    break;
                }
            }
            if shutdown_signal.load(Ordering::Relaxed) {
                break;
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
