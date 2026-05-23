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
    pub fn key(&self) -> &[u8] {
        match self {
            Self::Get { key } | Self::Set { key, .. } | Self::Delete { key } => key,
        }
    }
    pub fn value(&self) -> Option<&[u8]> {
        match self {
            Self::Get { .. } | Self::Delete { .. } => None,
            Self::Set { value, .. } => Some(value),
        }
    }
}

impl TryFrom<&str> for Command {
    type Error = CommandError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut iter = value.split_whitespace();
        let cmd = iter.next().ok_or(CommandError::NotEnoughParts)?;
        let cmd = cmd.to_lowercase();
        match cmd.as_str() {
            "get" => {
                let key = iter.next().ok_or(CommandError::NotEnoughParts)?;
                if iter.next().is_some() {
                    return Err(CommandError::TooManyParts);
                }
                Ok(Self::get(key))
            }
            "set" => {
                let (Some(key), Some(value)) = (iter.next(), iter.next()) else {
                    return Err(CommandError::NotEnoughParts);
                };
                if iter.next().is_some() {
                    return Err(CommandError::TooManyParts);
                }
                Ok(Self::set(key, value))
            }
            "del" => {
                let key = iter.next().ok_or(CommandError::NotEnoughParts)?;
                if iter.next().is_some() {
                    return Err(CommandError::TooManyParts);
                }
                Ok(Self::delete(key))
            }
            _ => Err(CommandError::UnrecognizedCommand),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn try_from_str_ok_get() {
        assert_eq!(Command::try_from("get key").unwrap(), Command::get(b"key"));
        assert_eq!(Command::try_from("GET key").unwrap(), Command::get(b"key"));
    }
    #[test]
    fn try_from_str_ok_set() {
        assert_eq!(
            Command::try_from("set key value").unwrap(),
            Command::set(b"key", b"value")
        );
        assert_eq!(
            Command::try_from("SET key value").unwrap(),
            Command::set(b"key", b"value")
        );
    }
    #[test]
    fn try_from_str_ok_del() {
        assert_eq!(
            Command::try_from("del key").unwrap(),
            Command::delete(b"key")
        );
        assert_eq!(
            Command::try_from("DEL key").unwrap(),
            Command::delete(b"key")
        );
    }
    #[test]
    fn try_from_str_err_get_too_many_parts() {
        assert!(matches!(
            Command::try_from("get key key"),
            Err(CommandError::TooManyParts)
        ));
    }
    #[test]
    fn try_from_str_err_set_too_many_parts() {
        assert!(matches!(
            Command::try_from("set key value value"),
            Err(CommandError::TooManyParts)
        ));
    }
    #[test]
    fn try_from_str_err_del_too_many_parts() {
        assert!(matches!(
            Command::try_from("del key key"),
            Err(CommandError::TooManyParts)
        ));
    }
    #[test]
    fn try_from_str_err_get_not_enough_parts() {
        assert!(matches!(
            Command::try_from("get"),
            Err(CommandError::NotEnoughParts)
        ));
    }
    #[test]
    fn try_from_str_err_set_not_enough_parts() {
        assert!(matches!(
            Command::try_from("set key"),
            Err(CommandError::NotEnoughParts)
        ));
        assert!(matches!(
            Command::try_from("set"),
            Err(CommandError::NotEnoughParts)
        ));
    }
    #[test]
    fn try_from_str_err_del_not_enough_parts() {
        assert!(matches!(
            Command::try_from("del"),
            Err(CommandError::NotEnoughParts)
        ));
    }
    #[test]
    fn try_from_str_err_unrecognized_command() {
        assert!(matches!(
            Command::try_from("foo"),
            Err(CommandError::UnrecognizedCommand)
        ));
    }
}
