use crate::{
    domain::command::{Command, CommandError},
    inbound::resp::frame::Frame,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandFromFrameError {
    #[error(transparent)]
    CommandError(#[from] CommandError),
    #[error("unexpected value; command is made of an array of bulk strings")]
    UnexpectedFrame,
}

impl TryFrom<Frame> for Command {
    type Error = CommandFromFrameError;
    fn try_from(value: Frame) -> Result<Self, Self::Error> {
        let Frame::Array(vec) = value else {
            return Err(CommandFromFrameError::UnexpectedFrame);
        };
        let mut iter = vec.into_iter();
        let Some(command_value) = iter.next() else {
            return Err(CommandError::NotEnoughParts.into());
        };
        match command_value {
            Frame::BulkString(command) => {
                let command = command.to_ascii_lowercase();
                match command.as_slice() {
                    b"get" => {
                        let key = iter.next().ok_or(CommandError::NotEnoughParts)?;
                        let Frame::BulkString(key) = key else {
                            return Err(CommandFromFrameError::UnexpectedFrame);
                        };
                        if iter.next().is_some() {
                            return Err(CommandError::TooManyParts.into());
                        }
                        Ok(Self::get(key))
                    }
                    b"set" => {
                        let (Some(key), Some(value)) = (iter.next(), iter.next()) else {
                            return Err(CommandError::NotEnoughParts.into());
                        };
                        let (Frame::BulkString(key), Frame::BulkString(value)) = (key, value)
                        else {
                            return Err(CommandFromFrameError::UnexpectedFrame);
                        };
                        if iter.next().is_some() {
                            return Err(CommandError::TooManyParts.into());
                        }
                        Ok(Self::set(key, value))
                    }
                    b"del" => {
                        let key = iter.next().ok_or(CommandError::NotEnoughParts)?;
                        let Frame::BulkString(key) = key else {
                            return Err(CommandFromFrameError::UnexpectedFrame);
                        };
                        if iter.next().is_some() {
                            return Err(CommandError::TooManyParts.into());
                        }
                        Ok(Self::delete(key))
                    }
                    b"ping" => Ok(Self::Ping),
                    _ => Err(CommandError::UnrecognizedCommand.into()),
                }
            }
            Frame::Array(_) => Err(CommandFromFrameError::UnexpectedFrame),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn try_from_value_ok_get() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"get".to_vec()),
                Frame::BulkString(b"key".to_vec()),
            ]))
            .unwrap(),
            Command::get(b"key")
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"GET".to_vec()),
                Frame::BulkString(b"key".to_vec()),
            ]))
            .unwrap(),
            Command::get(b"key")
        );
    }
    #[test]
    fn try_from_value_ok_set() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"set".to_vec()),
                Frame::BulkString(b"key".to_vec()),
                Frame::BulkString(b"value".to_vec()),
            ]))
            .unwrap(),
            Command::set(b"key", b"value")
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"SET".to_vec()),
                Frame::BulkString(b"key".to_vec()),
                Frame::BulkString(b"value".to_vec()),
            ]))
            .unwrap(),
            Command::set(b"key", b"value")
        );
    }
    #[test]
    fn try_from_value_ok_del() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"del".to_vec()),
                Frame::BulkString(b"key".to_vec()),
            ]))
            .unwrap(),
            Command::delete(b"key")
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"DEL".to_vec()),
                Frame::BulkString(b"key".to_vec()),
            ]))
            .unwrap(),
            Command::delete(b"key")
        );
    }
    #[test]
    fn try_from_value_err_get_too_many_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"get".to_vec()),
                Frame::BulkString(b"key".to_vec()),
                Frame::BulkString(b"key".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::TooManyParts
            ))
        ));
    }
    #[test]
    fn try_from_value_err_set_too_many_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"set".to_vec()),
                Frame::BulkString(b"key".to_vec()),
                Frame::BulkString(b"value".to_vec()),
                Frame::BulkString(b"value".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::TooManyParts
            ))
        ));
    }
    #[test]
    fn try_from_value_err_del_too_many_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"del".to_vec()),
                Frame::BulkString(b"key".to_vec()),
                Frame::BulkString(b"key".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::TooManyParts
            ))
        ));
    }
    #[test]
    fn try_from_value_err_get_not_enough_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"get".to_vec()),])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }
    #[test]
    fn try_from_value_err_set_not_enough_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"set".to_vec()),])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"set".to_vec()),
                Frame::BulkString(b"key".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }
    #[test]
    fn try_from_value_err_del_not_enough_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"get".to_vec()),])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }
    #[test]
    fn try_from_value_err_unrecognized_command() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"foo".to_vec())])),
            Err(CommandFromFrameError::CommandError(
                CommandError::UnrecognizedCommand
            ))
        ));
    }
    #[test]
    fn try_from_value_err_unexpected_value() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"get".to_vec()),
                Frame::Array(vec![Frame::BulkString(b"foo".to_vec())]),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
        assert!(matches!(
            Command::try_from(Frame::BulkString(b"get".to_vec())),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }
}
