use std::{num::ParseIntError, str::Utf8Error};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("unrecognized command")]
    UnrecognizedCommand,
    #[error("not enough parts")]
    NotEnoughParts,
    #[error("too many parts")]
    TooManyParts,
    #[error(transparent)]
    Utf8(#[from] Utf8Error),
    #[error(transparent)]
    ParseInt(#[from] ParseIntError),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Get { key: Vec<u8> },
    Set { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
    Ping { message: Option<Vec<u8>> },
    Exists { key: Vec<u8> },
    Expire { key: Vec<u8>, relative_ttl: u64 },
    ExpireAt { key: Vec<u8>, absolute_ttl: u64 },
    TTL { key: Vec<u8> },
    Persist { key: Vec<u8> },
}

impl Command {
    pub fn get(key: impl Into<Vec<u8>>) -> Self {
        Self::Get { key: key.into() }
    }

    pub fn set(key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self::Set {
            key: key.into(),
            value: value.into(),
        }
    }

    pub fn delete(key: impl Into<Vec<u8>>) -> Self {
        Self::Delete { key: key.into() }
    }

    pub fn ping(message: Option<Vec<u8>>) -> Self {
        Self::Ping { message }
    }

    pub fn exists(key: impl Into<Vec<u8>>) -> Self {
        Self::Exists { key: key.into() }
    }

    pub fn expire(key: impl Into<Vec<u8>>, relative_ttl: u64) -> Self {
        Self::Expire {
            key: key.into(),
            relative_ttl,
        }
    }

    pub fn expire_at(key: impl Into<Vec<u8>>, absolute_ttl: u64) -> Self {
        Self::ExpireAt {
            key: key.into(),
            absolute_ttl,
        }
    }

    pub fn ttl(key: impl Into<Vec<u8>>) -> Self {
        Self::TTL { key: key.into() }
    }

    pub fn persist(key: impl Into<Vec<u8>>) -> Self {
        Self::Persist { key: key.into() }
    }
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
            Command::TTL {
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
