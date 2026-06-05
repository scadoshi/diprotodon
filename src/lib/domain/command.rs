//! Domain command type — the parsed, validated representation of a client request.
//!
//! [`Command`] is the boundary between the wire/RESP layer and the cache. RESP frames are
//! parsed into a `Command` in `inbound::resp::command`; `inbound::session::execute` then
//! dispatches on the variant to drive `Cache`. The enum is wire-agnostic: it carries
//! exactly the data each operation needs and nothing about how it arrived.

use std::{num::ParseIntError, str::Utf8Error};
use thiserror::Error;

/// Errors produced while building a [`Command`] from already-parsed RESP frames.
///
/// These are *semantic* errors (the frames parsed fine but the contents are wrong for
/// the requested command). Frame-shape errors live one layer below in `FrameError`.
#[derive(Error, Debug)]
pub enum CommandError {
    /// First bulk string didn't match any known command verb (e.g. `b"foo"`).
    #[error("unrecognized command")]
    UnrecognizedCommand,
    /// Command requires more arguments than were supplied (e.g. `GET` with no key).
    #[error("not enough parts")]
    NotEnoughParts,
    /// Command got more arguments than it accepts (e.g. `GET foo bar`).
    #[error("too many parts")]
    TooManyParts,
    /// A numeric argument's bytes weren't valid UTF-8 (e.g. `EXPIRE foo <binary>`).
    #[error(transparent)]
    Utf8(#[from] Utf8Error),
    /// A numeric argument was valid UTF-8 but didn't parse as the required integer
    /// type (non-digits, negative for `u64`, overflow, etc.).
    #[error(transparent)]
    ParseInt(#[from] ParseIntError),
}

/// A validated client command, ready to be executed against the cache.
///
/// Variants are one-to-one with the RESP commands the server understands. Keys and
/// values are `Vec<u8>` (not `String`) so the server stays binary-safe — bulk strings on
/// the wire can be arbitrary bytes.
///
/// Construct via the lowercase helper methods (`Command::get`, `Command::set`, …) or
/// by destructuring/pattern-matching the variants directly.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `GET <key>` — fetch the value for `key`, or nil if absent/expired.
    Get { key: Vec<u8> },
    /// `SET <key> <value>` — insert or replace `key`'s value. Ttl handling is up to
    /// the executor; this variant carries no Ttl on its own.
    Set { key: Vec<u8>, value: Vec<u8> },
    /// `DEL <key>` — remove `key` if present.
    Delete { key: Vec<u8> },
    /// `PING` (no arg) or `PING <message>` — health check; echoes `message` back as a
    /// bulk string when present, otherwise replies `+PONG`.
    Ping { message: Option<Vec<u8>> },
    /// `EXISTS <key>` — `1` if `key` is present (and not expired), `0` otherwise.
    Exists { key: Vec<u8> },
    /// `EXPIRE <key> <seconds>` — set a relative Ttl on `key`. `relative_ttl` is
    /// seconds-from-now; the cache layer converts to absolute UNIX seconds.
    Expire { key: Vec<u8>, relative_ttl: u64 },
    /// `EXPIREAT <key> <timestamp>` — set an absolute Ttl on `key` as UNIX seconds.
    /// A past timestamp deletes the key immediately (matches real Redis).
    ExpireAt { key: Vec<u8>, absolute_ttl: u64 },
    /// `Ttl <key>` — query remaining Ttl in seconds. Replies `:-2` if missing, `:-1`
    /// if `key` has no Ttl, `:n` for seconds remaining.
    Ttl { key: Vec<u8> },
    /// `PERSIST <key>` — remove the Ttl from `key` (keep the value). Replies `:1` if
    /// a Ttl was removed, `:0` if `key` is missing or already had no Ttl.
    Persist { key: Vec<u8> },
}

impl Command {
    /// Build a [`Command::Get`] from anything that converts into `Vec<u8>`.
    pub fn get(key: impl Into<Vec<u8>>) -> Self {
        Self::Get { key: key.into() }
    }

    /// Build a [`Command::Set`] from convertible key + value bytes.
    pub fn set(key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self::Set {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Build a [`Command::Delete`] from a convertible key.
    pub fn delete(key: impl Into<Vec<u8>>) -> Self {
        Self::Delete { key: key.into() }
    }

    /// Build a [`Command::Ping`]. Pass `None` for plain `PING`, `Some(bytes)` to echo
    /// a message back to the client.
    pub fn ping(message: Option<Vec<u8>>) -> Self {
        Self::Ping { message }
    }

    /// Build a [`Command::Exists`] from a convertible key.
    pub fn exists(key: impl Into<Vec<u8>>) -> Self {
        Self::Exists { key: key.into() }
    }

    /// Build a [`Command::Expire`] with a relative Ttl in seconds-from-now.
    pub fn expire(key: impl Into<Vec<u8>>, relative_ttl: u64) -> Self {
        Self::Expire {
            key: key.into(),
            relative_ttl,
        }
    }

    /// Build a [`Command::ExpireAt`] with an absolute Ttl in UNIX seconds.
    pub fn expire_at(key: impl Into<Vec<u8>>, absolute_ttl: u64) -> Self {
        Self::ExpireAt {
            key: key.into(),
            absolute_ttl,
        }
    }

    /// Build a [`Command::Ttl`] from a convertible key.
    pub fn ttl(key: impl Into<Vec<u8>>) -> Self {
        Self::Ttl { key: key.into() }
    }

    /// Build a [`Command::Persist`] from a convertible key.
    pub fn persist(key: impl Into<Vec<u8>>) -> Self {
        Self::Persist { key: key.into() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MutatingCommand {
    Set { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
    Expire { key: Vec<u8>, relative_ttl: u64 },
    ExpireAt { key: Vec<u8>, absolute_ttl: u64 },
    Persist { key: Vec<u8> },
}

impl MutatingCommand {
    pub fn from_command(command: Command) -> Option<Self> {
        match command {
            Command::Get { .. }
            | Command::Ping { .. }
            | Command::Exists { .. }
            | Command::Ttl { .. } => None,
            Command::Set { key, value } => Some(Self::Set { key, value }),
            Command::Delete { key } => Some(Self::Delete { key }),
            Command::Expire { key, relative_ttl } => Some(Self::Expire { key, relative_ttl }),
            Command::ExpireAt { key, absolute_ttl } => Some(Self::ExpireAt { key, absolute_ttl }),
            Command::Persist { key } => Some(Self::Persist { key }),
        }
    }
}

#[derive(Debug)]
pub enum TtlOutcome {
    KeyNotFound,
    TtlNotFound,
    Some(u64),
}

#[derive(Debug)]
pub enum CommandOutcome {
    Value(Option<Vec<u8>>),
    Ok,
    Bool(bool),
    Ttl(TtlOutcome),
    Pong(Option<Vec<u8>>),
    Integer(i64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_constructor() {
        assert_eq!(
            Command::get("foo"),
            Command::Get {
                key: b"foo".to_vec()
            }
        );
    }

    #[test]
    fn set_constructor() {
        assert_eq!(
            Command::set("foo", "bar"),
            Command::Set {
                key: b"foo".to_vec(),
                value: b"bar".to_vec(),
            }
        );
    }

    #[test]
    fn delete_constructor() {
        assert_eq!(
            Command::delete("foo"),
            Command::Delete {
                key: b"foo".to_vec()
            }
        );
    }

    #[test]
    fn ping_constructor_without_message() {
        assert_eq!(Command::ping(None), Command::Ping { message: None });
    }

    #[test]
    fn ping_constructor_with_message() {
        assert_eq!(
            Command::ping(Some(b"hello".to_vec())),
            Command::Ping {
                message: Some(b"hello".to_vec())
            }
        );
    }

    #[test]
    fn exists_constructor() {
        assert_eq!(
            Command::exists("foo"),
            Command::Exists {
                key: b"foo".to_vec()
            }
        );
    }

    #[test]
    fn expire_constructor() {
        assert_eq!(
            Command::expire("foo", 123),
            Command::Expire {
                key: b"foo".to_vec(),
                relative_ttl: 123
            }
        );
    }

    #[test]
    fn expire_at_constructor() {
        assert_eq!(
            Command::expire_at("foo", 1_700_000_000),
            Command::ExpireAt {
                key: b"foo".to_vec(),
                absolute_ttl: 1_700_000_000
            }
        );
    }

    #[test]
    fn ttl_constructor() {
        assert_eq!(
            Command::ttl("foo"),
            Command::Ttl {
                key: b"foo".to_vec()
            }
        );
    }

    #[test]
    fn persist_constructor() {
        assert_eq!(
            Command::persist("foo"),
            Command::Persist {
                key: b"foo".to_vec()
            }
        );
    }
}
