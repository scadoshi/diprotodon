//! Append-only command log — the write-ahead log half of the persister.
//!
//! The AOF *is* the RESP wire format: each logged command is encoded with the same
//! `From<WriteCommand> for Frame` + `Frame::write_to` used on the network path, so the
//! file is byte-for-byte what a client would have sent. That symmetry means replay needs
//! no special decoder — it reuses `Frame::parse_one` + `Command::try_from`, the exact
//! inbound parsing path. The log is, in effect, a transcript of every mutation; replay is
//! "re-send that transcript."

use crate::{
    domain::{
        cache::{Cache, CacheError},
        command::{Command, cache::write::WriteCommand},
        ports::RepositoryError,
    },
    outbound::persister::persister_inner::PersisterInner,
    resp::{
        command::CommandFromFrameError,
        frame::{Frame, FrameError},
    },
};
use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Read},
    ops::Deref,
};
use thiserror::Error;

type IoError = std::io::Error;

/// Failure on the AOF write or replay path. Boxed into [`RepositoryError`] at the boundary.
#[derive(Debug, Error)]
pub enum AofError {
    /// File I/O failed.
    #[error(transparent)]
    Io(#[from] IoError),
    /// The writer mutex was poisoned (a writer panicked while holding it).
    #[error("mutex poisoned")]
    MutexPoisoned,
    /// A replayed command failed to apply to the cache.
    #[error(transparent)]
    Cache(#[from] CacheError),
    /// A replayed frame parsed but didn't lift into a known command — corruption or
    /// version skew. Fatal: the log is our own, so an uninterpretable entry means the
    /// rebuild can't be trusted.
    #[error(transparent)]
    Command(#[from] CommandFromFrameError),
    /// A frame failed to parse mid-stream (hard corruption, distinct from a torn tail).
    #[error(transparent)]
    Frame(#[from] FrameError),
}

impl From<AofError> for RepositoryError {
    fn from(value: AofError) -> Self {
        RepositoryError::Generic(Box::new(value))
    }
}

/// The append-only log. Newtype over `PersisterInner` (shared writer + path); `Deref`
/// exposes the inner handle to the methods below.
#[derive(Debug, Clone)]
pub struct Aof(PersisterInner);

impl Deref for Aof {
    type Target = PersisterInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<PersisterInner> for Aof {
    fn from(value: PersisterInner) -> Self {
        Self(value)
    }
}

impl Aof {
    /// Encode one mutation as a RESP frame and append it to the log. Holds the writer lock
    /// across the write so a frame is never interleaved with another thread's append.
    pub fn append(&self, command: WriteCommand) -> Result<(), AofError> {
        let mut guard = self.writer.lock().map_err(|_| AofError::MutexPoisoned)?;
        Frame::from(command).write_to(&mut *guard)?;
        Ok(())
    }

    /// Rebuild cache state by replaying the log: parse each frame, lift it to a command,
    /// apply it *in-memory only* (no re-logging).
    ///
    /// Reads the whole file, then walks it frame-by-frame. Stops on a trailing
    /// [`Incomplete`](FrameError::Incomplete) frame — the expected torn tail from a crash
    /// mid-append — keeping everything parsed so far. Any *other* parse failure or an
    /// unknown command is fatal (returns `Err`), because in our own log those signal real
    /// corruption, not a normal partial write. Runs at startup before clients connect, so
    /// per-command locking inside `execute` is fine — no concurrency to coordinate.
    pub fn replay(&self, cache: &Cache) -> Result<(), AofError> {
        let aof = {
            let mut aof = Vec::<u8>::new();
            File::open(&*self.path)?.read_to_end(&mut aof)?;
            aof
        };
        let mut bytes = aof.as_slice();
        loop {
            match Frame::parse_one(bytes) {
                Ok((frame, remainder)) => {
                    bytes = remainder;
                    match Command::try_from(frame) {
                        Ok(Command::Cache(cc)) => {
                            cache.execute(&cc)?;
                        }
                        Ok(Command::Channel(_)) => {}
                        Ok(Command::Ping { .. }) => {}
                        Err(e) => return Err(e.into()),
                    }
                }
                Err(FrameError::Incomplete) => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<(), AofError> {
        let mut guard = self.writer.lock().map_err(|_| AofError::MutexPoisoned)?;
        let cleared = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&*self.path)?;
        *guard = BufWriter::new(cleared);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::cache::Entry, test_support::TempPath};
    use std::{fs, io::Write};

    fn fresh() -> (Aof, TempPath) {
        let temp = TempPath::new("aof");
        let inner = PersisterInner::try_from(temp.path.clone()).unwrap();
        (Aof::from(inner), temp)
    }

    /// Flush the BufWriter so bytes actually land on disk for the next read.
    fn flush(aof: &Aof) {
        aof.writer.lock().unwrap().flush().unwrap();
    }

    // ---------- append ----------

    // #[test]
    // fn append_writes_resp_frame_to_disk() {
    //     let (aof, t) = fresh();
    //     aof.append(WriteCommand::Set {
    //         key: b"foo".to_vec(),
    //         value: b"bar".to_vec(),
    //     })
    //     .unwrap();
    //     flush(&aof);
    //     let bytes = fs::read(&t.path).unwrap();
    //     assert_eq!(bytes, b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\nbar\r\n");
    // }

    // #[test]
    // fn append_multiple_commands_concatenates() {
    //     let (aof, t) = fresh();
    //     aof.append(WriteCommand::Set {
    //         key: b"a".to_vec(),
    //         value: b"1".to_vec(),
    //     })
    //     .unwrap();
    //     aof.append(WriteCommand::Delete { key: b"a".to_vec() })
    //         .unwrap();
    //     flush(&aof);
    //     let bytes = fs::read(&t.path).unwrap();
    //     let expected = b"*3\r\n$3\r\nSET\r\n$1\r\na\r\n$1\r\n1\r\n*2\r\n$3\r\nDEL\r\n$1\r\na\r\n";
    //     assert_eq!(bytes, expected);
    // }

    // ---------- replay ----------

    #[test]
    fn replay_empty_file_is_noop() {
        let (aof, _t) = fresh();
        let cache = Cache::default();
        aof.replay(&cache).unwrap();
        assert_eq!(cache.lock().unwrap().len(), 0);
    }

    // #[test]
    // fn replay_applies_set_to_cache() {
    //     let (aof, _t) = fresh();
    //     aof.append(WriteCommand::Set {
    //         key: b"foo".to_vec(),
    //         value: b"bar".to_vec(),
    //     })
    //     .unwrap();
    //     flush(&aof);
    //     let cache = Cache::default();
    //     aof.replay(&cache).unwrap();
    //     assert_eq!(cache.get("foo").unwrap(), Some(Entry::new("bar", None)));
    // }

    // #[test]
    // fn replay_applies_commands_in_order() {
    //     let (aof, _t) = fresh();
    //     aof.append(WriteCommand::Set {
    //         key: b"foo".to_vec(),
    //         value: b"old".to_vec(),
    //     })
    //     .unwrap();
    //     aof.append(WriteCommand::Set {
    //         key: b"foo".to_vec(),
    //         value: b"new".to_vec(),
    //     })
    //     .unwrap();
    //     aof.append(WriteCommand::Delete {
    //         key: b"gone".to_vec(),
    //     })
    //     .unwrap();
    //     flush(&aof);
    //     let cache = Cache::default();
    //     aof.replay(&cache).unwrap();
    //     assert_eq!(cache.get("foo").unwrap(), Some(Entry::new("new", None)));
    //     assert_eq!(cache.get("gone").unwrap(), None);
    // }

    // Proves an EXPIREAT in the log survives the round-trip (encode -> disk -> parse ->
    // execute) and is applied on replay. The key is seeded directly rather than via an
    // appended SET, because SET -> Frame encoding is still a todo!() (the in-progress
    // set-options work); the original SET-then-EXPIREAT form returns with the other AOF
    // SET tests once that lands.
    #[test]
    fn replay_applies_expire_at() {
        let (aof, _t) = fresh();
        aof.append(WriteCommand::ExpireAt {
            key: b"foo".to_vec(),
            absolute_ttl: u64::MAX,
        })
        .unwrap();
        flush(&aof);

        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        aof.replay(&cache).unwrap();
        assert_eq!(cache.get_absolute_ttl("foo").unwrap(), Some(Some(u64::MAX)));
    }

    // #[test]
    // fn replay_trailing_partial_frame_is_tolerated() {
    //     // Incomplete trailing frame should stop replay cleanly (Incomplete is the EOF signal).
    //     let (aof, t) = fresh();
    //     aof.append(WriteCommand::Set {
    //         key: b"foo".to_vec(),
    //         value: b"bar".to_vec(),
    //     })
    //     .unwrap();
    //     flush(&aof);
    //     // Append a partial frame directly to the file behind the BufWriter.
    //     let mut handle = OpenOptions::new().append(true).open(&t.path).unwrap();
    //     handle.write_all(b"*3\r\n$3\r\nSET\r\n").unwrap();
    //     drop(handle);
    //
    //     let cache = Cache::default();
    //     aof.replay(&cache).unwrap();
    //     assert_eq!(cache.get("foo").unwrap(), Some(Entry::new("bar", None)));
    // }

    #[test]
    fn replay_malformed_frame_errors() {
        let (aof, t) = fresh();
        let mut handle = OpenOptions::new().append(true).open(&t.path).unwrap();
        handle.write_all(b"?bogus\r\n").unwrap();
        drop(handle);
        let cache = Cache::default();
        assert!(matches!(aof.replay(&cache), Err(AofError::Frame(_))));
    }

    // ---------- clear ----------

    // #[test]
    // fn clear_truncates_file() {
    //     let (aof, t) = fresh();
    //     aof.append(WriteCommand::Set {
    //         key: b"foo".to_vec(),
    //         value: b"bar".to_vec(),
    //     })
    //     .unwrap();
    //     flush(&aof);
    //     aof.clear().unwrap();
    //     assert_eq!(fs::read(&t.path).unwrap().len(), 0);
    // }

    //     #[test]
    //     fn clear_allows_subsequent_appends() {
    //         let (aof, _t) = fresh();
    //         aof.append(WriteCommand::Set {
    //             key: b"old".to_vec(),
    //             value: b"v".to_vec(),
    //         })
    //         .unwrap();
    //         flush(&aof);
    //         aof.clear().unwrap();
    //         aof.append(WriteCommand::Set {
    //             key: b"new".to_vec(),
    //             value: b"v".to_vec(),
    //         })
    //         .unwrap();
    //         flush(&aof);
    //         let cache = Cache::default();
    //         aof.replay(&cache).unwrap();
    //         assert_eq!(cache.get("old").unwrap(), None);
    //         assert_eq!(cache.get("new").unwrap(), Some(Entry::new("v", None)));
    //     }
}
