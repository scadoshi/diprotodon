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

#[derive(Debug)]
pub enum Command {
    Get { key: String },
    Set { key: String, value: String },
    Delete { key: String },
}

impl Command {
    pub fn get(key: impl Into<String>) -> Self {
        Self::Get { key: key.into() }
    }
    pub fn set(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Set {
            key: key.into(),
            value: value.into(),
        }
    }
    pub fn delete(key: impl Into<String>) -> Self {
        Self::Delete { key: key.into() }
    }
    pub fn key(&self) -> &str {
        match self {
            Self::Get { key } | Self::Set { key, .. } | Self::Delete { key } => key,
        }
    }
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Get { .. } | Self::Delete { .. } => None,
            Self::Set { value, .. } => Some(value),
        }
    }
}

impl TryFrom<&str> for Command {
    type Error = CommandError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = value.trim();
        let Some((command_str, arguments_str)) = value.split_once(' ') else {
            return Err(CommandError::NotEnoughParts);
        };
        if command_str == "get" || command_str == "g" {
            if arguments_str.split_once(' ').is_some() {
                return Err(CommandError::TooManyParts);
            }
            Ok(Self::get(arguments_str))
        } else if command_str == "set" || command_str == "s" {
            let Some((key, value)) = arguments_str.split_once(' ') else {
                return Err(CommandError::NotEnoughParts);
            };
            if value.split_once(' ').is_some() {
                return Err(CommandError::TooManyParts);
            }
            Ok(Self::set(key, value))
        } else if command_str == "delete" || command_str == "del" || command_str == "d" {
            if arguments_str.split_once(' ').is_some() {
                return Err(CommandError::TooManyParts);
            }
            Ok(Self::delete(arguments_str))
        } else {
            Err(CommandError::UnrecognizedCommand)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_str_ok() {
        assert!(matches!(
            Command::try_from("get key"),
            Ok(Command::Get { .. })
        ));
        assert!(matches!(
            Command::try_from("g key"),
            Ok(Command::Get { .. })
        ));
        assert!(matches!(
            Command::try_from("set key value"),
            Ok(Command::Set { .. })
        ));
        assert!(matches!(
            Command::try_from("s key value"),
            Ok(Command::Set { .. })
        ));
        assert!(matches!(
            Command::try_from("delete key"),
            Ok(Command::Delete { .. })
        ));
        assert!(matches!(
            Command::try_from("del key"),
            Ok(Command::Delete { .. })
        ));
        assert!(matches!(
            Command::try_from("d key"),
            Ok(Command::Delete { .. })
        ));
    }

    #[test]
    fn try_from_str_err() {
        assert!(matches!(
            Command::try_from("get key key"),
            Err(CommandError::TooManyParts)
        ));
        assert!(matches!(
            Command::try_from("set key value value"),
            Err(CommandError::TooManyParts)
        ));
        assert!(matches!(
            Command::try_from("delete key key"),
            Err(CommandError::TooManyParts)
        ));
        assert!(matches!(
            Command::try_from("get"),
            Err(CommandError::NotEnoughParts)
        ));
        assert!(matches!(
            Command::try_from("set key"),
            Err(CommandError::NotEnoughParts)
        ));
        assert!(matches!(
            Command::try_from("set"),
            Err(CommandError::NotEnoughParts)
        ));
        assert!(matches!(
            Command::try_from("delete"),
            Err(CommandError::NotEnoughParts)
        ));
        assert!(matches!(
            Command::try_from("unrecognized key"),
            Err(CommandError::UnrecognizedCommand)
        ));
    }
}
