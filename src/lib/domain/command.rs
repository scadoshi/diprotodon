use thiserror::Error;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("unrecognized command")]
    UnrecognizedCommand,
    #[error("not enough parts")]
    NotEnoughParts,
    #[error("too many parts")]
    TooManyParts,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Get { key: Vec<u8> },
    Set { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
    Ping,
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
    pub fn key(&self) -> Option<&[u8]> {
        match self {
            Self::Get { key } | Self::Set { key, .. } | Self::Delete { key } => Some(key),
            Self::Ping => None,
        }
    }
    pub fn value(&self) -> Option<&[u8]> {
        match self {
            Self::Get { .. } | Self::Delete { .. } | Self::Ping => None,
            Self::Set { value, .. } => Some(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_constructor() {
        assert_eq!(
            Command::get("key"),
            Command::Get {
                key: b"key".to_vec()
            }
        );
    }

    #[test]
    fn set_constructor() {
        assert_eq!(
            Command::set("key", "value"),
            Command::Set {
                key: b"key".to_vec(),
                value: b"value".to_vec(),
            }
        );
    }

    #[test]
    fn delete_constructor() {
        assert_eq!(
            Command::delete("key"),
            Command::Delete {
                key: b"key".to_vec()
            }
        );
    }

    #[test]
    fn key_get() {
        assert_eq!(Command::get("key").key(), Some(b"key".as_slice()));
    }

    #[test]
    fn key_set() {
        assert_eq!(Command::set("key", "value").key(), Some(b"key".as_slice()));
    }

    #[test]
    fn key_delete() {
        assert_eq!(Command::delete("key").key(), Some(b"key".as_slice()));
    }

    #[test]
    fn key_ping() {
        assert_eq!(Command::Ping.key(), None);
    }

    #[test]
    fn value_get() {
        assert_eq!(Command::get("key").value(), None);
    }

    #[test]
    fn value_set() {
        assert_eq!(
            Command::set("key", "value").value(),
            Some(b"value".as_slice())
        );
    }

    #[test]
    fn value_delete() {
        assert_eq!(Command::delete("key").value(), None);
    }

    #[test]
    fn value_ping() {
        assert_eq!(Command::Ping.value(), None);
    }
}
