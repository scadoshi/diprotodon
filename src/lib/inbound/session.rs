//! Per-connection session — the REPL that drives a single TCP client.
//!
//! [`Session`] owns the buffered reader/writer for one connection and a
//! [`CacheService`] handle. Its loop is: read bytes → parse a frame → lift it to a
//! [`Command`] → hand it to the service → map the returned outcome to a [`Reply`] and
//! write it back. Malformed frames and unknown commands produce a `-ERR ...\r\n` reply
//! but do not kill the session.
//!
//! The session is a pure *streamer* — it moves bytes in and out. Command execution
//! (cache mutation + persistence) lives behind the service; the session only adds RESP
//! framing on the way in and the outcome→reply translation on the way out.
//!
//! Generic over `R: Read` / `W: Write` so tests can drive a session with a
//! `Cursor<Vec<u8>>` reader and a `Vec<u8>` writer — no actual TCP needed.

use crate::{
    domain::{
        command::Command,
        ports::{CacheService, ServiceError},
    },
    resp::{
        frame::{Frame, FrameError},
        reply::{Reply, SimpleInner},
    },
};
use std::{
    io::{BufReader, BufWriter, ErrorKind, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use thiserror::Error;

/// Fatal errors from the REPL loop. Non-fatal errors (bad frames, unknown commands)
/// are converted to RESP error replies inside the loop and do not surface here.
#[derive(Debug, Error)]
pub enum SessionError {
    /// I/O failed on the socket — the client probably went away.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Cache layer error during command execution.
    #[error(transparent)]
    Service(#[from] ServiceError),
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
pub struct Session<R: Read, W: Write, CS: CacheService> {
    id: u32,
    reader: SessionReader<R>,
    writer: BufWriter<W>,
    cache_service: CS,
}

impl<R: Read, W: Write, CS: CacheService> Session<R, W, CS> {
    /// Build a session for a single connection. `id` is just a numeric tag used in
    /// connection-lifecycle logs; the cache is shared across all sessions via its
    /// internal `Arc`.
    pub fn new(id: u32, reader: R, writer: W, cache_service: CS) -> Self {
        let writer = BufWriter::new(writer);
        let reader = SessionReader::new(reader);
        Self {
            id,
            reader,
            writer,
            cache_service,
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
            match self.reader.parse_frame() {
                Ok(frame) => return Ok(Some(frame)),
                Err(FrameError::Incomplete) => {
                    if self.reader.read()? == 0 {
                        return Ok(None);
                    }
                    continue;
                }
                Err(e) => {
                    Reply::SimpleError(SimpleInner::sanitized(format!("ERR {}", e)))
                        .write_to(&mut self.writer)?;
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
            match self.get_frame()?.map(Command::try_from) {
                Some(Ok(cmd)) => return Ok(Some(cmd)),
                Some(Err(e)) => {
                    Reply::SimpleError(SimpleInner::sanitized(format!("ERR {}", e)))
                        .write_to(&mut self.writer)?;
                    self.writer.flush()?;
                    continue;
                }
                None => return Ok(None),
            }
        }
    }

    pub fn execute(&mut self, command: Command) -> Result<(), SessionError> {
        let reply = Reply::from(self.cache_service.execute_logged(command)?);
        for _ in 0..10 {
            match reply.write_to(&mut self.writer) {
                Ok(_) => break,
                Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }
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
    pub fn repl(&mut self, shutdown_signal: Arc<AtomicBool>) -> Result<(), SessionError> {
        loop {
            if shutdown_signal.load(Ordering::Relaxed) {
                break;
            }
            match self.get_command() {
                Ok(Some(cmd)) => self.execute(cmd)?,
                Ok(None) => {
                    println!("client {} disconnected", self.id);
                    break;
                }
                Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                    continue;
                }
                Err(e) => return Err(e.into()),
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

    // ---------- execute / get_command ----------

    use crate::{
        domain::{cache::Cache, command::MutatingCommand, service::Service},
        test_support::{RecordingRepo, SharedWriter},
    };

    fn build(
        input: &[u8],
    ) -> (
        Session<Cursor<Vec<u8>>, SharedWriter, Service<RecordingRepo>>,
        Cache,
        RecordingRepo,
        SharedWriter,
    ) {
        let cache = Cache::default();
        let repo = RecordingRepo::default();
        let service = Service::new(cache.clone(), repo.clone());
        let writer = SharedWriter::default();
        let writer_handle = writer.clone();
        let session = Session::new(0, Cursor::new(input.to_vec()), writer, service);
        (session, cache, repo, writer_handle)
    }

    fn flush(session: &mut Session<Cursor<Vec<u8>>, SharedWriter, Service<RecordingRepo>>) {
        session.writer.flush().unwrap();
    }

    // ---------- execute: per-variant wire bytes ----------

    #[test]
    fn execute_set_writes_ok_and_appends_to_log() {
        let (mut s, cache, repo, written) = build(b"");
        s.execute(Command::set("foo", "bar")).unwrap();
        flush(&mut s);
        assert_eq!(&written.bytes()[..], b"+OK\r\n");
        assert!(cache.contains("foo").unwrap());
        assert_eq!(repo.appended.lock().unwrap().len(), 1);
    }

    #[test]
    fn execute_get_hit_writes_bulk_string() {
        let (mut s, cache, _, written) = build(b"");
        cache
            .insert("foo", crate::domain::cache::Entry::new("bar", None))
            .unwrap();
        s.execute(Command::get("foo")).unwrap();
        flush(&mut s);
        assert_eq!(&written.bytes()[..], b"$3\r\nbar\r\n");
    }

    #[test]
    fn execute_get_miss_writes_null_bulk() {
        let (mut s, _, _, written) = build(b"");
        s.execute(Command::get("missing")).unwrap();
        flush(&mut s);
        assert_eq!(&written.bytes()[..], b"$-1\r\n");
    }

    #[test]
    fn execute_delete_hit_writes_integer_one() {
        let (mut s, cache, _, written) = build(b"");
        cache
            .insert("foo", crate::domain::cache::Entry::new("bar", None))
            .unwrap();
        s.execute(Command::delete("foo")).unwrap();
        flush(&mut s);
        assert_eq!(&written.bytes()[..], b":1\r\n");
    }

    #[test]
    fn execute_delete_miss_writes_integer_zero() {
        let (mut s, _, _, written) = build(b"");
        s.execute(Command::delete("foo")).unwrap();
        flush(&mut s);
        assert_eq!(&written.bytes()[..], b":0\r\n");
    }

    #[test]
    fn execute_ping_without_message_writes_pong() {
        let (mut s, _, _, written) = build(b"");
        s.execute(Command::ping(None)).unwrap();
        flush(&mut s);
        assert_eq!(&written.bytes()[..], b"+PONG\r\n");
    }

    #[test]
    fn execute_ping_with_message_writes_simple_string_of_message() {
        let (mut s, _, _, written) = build(b"");
        s.execute(Command::ping(Some(b"hi".to_vec()))).unwrap();
        flush(&mut s);
        assert_eq!(&written.bytes()[..], b"+hi\r\n");
    }

    #[test]
    fn execute_exists_hit_writes_integer_one() {
        let (mut s, cache, _, written) = build(b"");
        cache
            .insert("foo", crate::domain::cache::Entry::new("bar", None))
            .unwrap();
        s.execute(Command::exists("foo")).unwrap();
        flush(&mut s);
        assert_eq!(&written.bytes()[..], b":1\r\n");
    }

    #[test]
    fn execute_ttl_missing_writes_negative_two() {
        let (mut s, _, _, written) = build(b"");
        s.execute(Command::ttl("missing")).unwrap();
        flush(&mut s);
        assert_eq!(&written.bytes()[..], b":-2\r\n");
    }

    #[test]
    fn execute_ttl_no_ttl_writes_negative_one() {
        let (mut s, cache, _, written) = build(b"");
        cache
            .insert("foo", crate::domain::cache::Entry::new("bar", None))
            .unwrap();
        s.execute(Command::ttl("foo")).unwrap();
        flush(&mut s);
        assert_eq!(&written.bytes()[..], b":-1\r\n");
    }

    #[test]
    fn execute_set_appends_set_to_log() {
        let (mut s, _, repo, _) = build(b"");
        s.execute(Command::set("foo", "bar")).unwrap();
        let log = repo.appended.lock().unwrap();
        assert!(matches!(
            &log[0],
            MutatingCommand::Set { key, value } if key == b"foo" && value == b"bar"
        ));
    }

    #[test]
    fn execute_get_does_not_append_to_log() {
        let (mut s, _, repo, _) = build(b"");
        s.execute(Command::get("foo")).unwrap();
        assert!(repo.appended.lock().unwrap().is_empty());
    }

    // ---------- get_command ----------

    #[test]
    fn get_command_returns_parsed_command() {
        let (mut s, _, _, _) = build(b"*2\r\n$3\r\nGET\r\n$3\r\nfoo\r\n");
        let cmd = s.get_command().unwrap();
        assert_eq!(cmd, Some(Command::get(b"foo")));
    }

    #[test]
    fn get_command_skips_unknown_verb_and_writes_err() {
        let input = b"*1\r\n$3\r\nfoo\r\n*2\r\n$3\r\nGET\r\n$3\r\nbar\r\n";
        let (mut s, _, _, written) = build(input);
        let cmd = s.get_command().unwrap();
        flush(&mut s);
        assert_eq!(cmd, Some(Command::get(b"bar")));
        let bytes = written.bytes();
        assert!(
            bytes.starts_with(b"-ERR "),
            "expected -ERR prefix, got {:?}",
            std::str::from_utf8(&bytes)
        );
    }

    #[test]
    fn get_command_dispatches_multiple_commands_in_order() {
        let input = b"*1\r\n$4\r\nPING\r\n*2\r\n$3\r\nGET\r\n$3\r\nfoo\r\n";
        let (mut s, _, _, _) = build(input);
        assert_eq!(s.get_command().unwrap(), Some(Command::ping(None)));
        assert_eq!(s.get_command().unwrap(), Some(Command::get(b"foo")));
    }

    #[test]
    fn get_command_eof_returns_none() {
        let (mut s, _, _, _) = build(b"");
        assert_eq!(s.get_command().unwrap(), None);
    }

    #[test]
    fn get_command_eof_after_valid_command_returns_none() {
        let (mut s, _, _, _) = build(b"*1\r\n$4\r\nPING\r\n");
        assert_eq!(s.get_command().unwrap(), Some(Command::ping(None)));
        assert_eq!(s.get_command().unwrap(), None);
    }
}
