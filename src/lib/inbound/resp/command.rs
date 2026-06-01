//! Lifts a parsed RESP [`Frame`] into a domain [`Command`].
//!
//! A valid command on the wire is always an array of bulk strings: the first is the
//! verb (case-insensitive), the rest are arguments. Anything else — a bare bulk string,
//! an array whose first element is an array, an inner array where a key is expected —
//! is a [`CommandFromFrameError::UnexpectedFrame`]. Verb-level errors (wrong arity,
//! non-numeric TTL, unknown verb) surface as [`CommandError`].

use crate::{
    domain::command::{Command, CommandError},
    inbound::resp::frame::Frame,
};
use thiserror::Error;

/// Errors produced while turning a [`Frame`] into a [`Command`].
#[derive(Debug, Error)]
pub enum CommandFromFrameError {
    /// Verb-level semantic error from the domain layer (wrong arity, unknown verb,
    /// numeric arg failed to parse, etc.).
    #[error(transparent)]
    CommandError(#[from] CommandError),
    /// The outer frame wasn't an array of bulk strings, or a nested frame appeared
    /// where the protocol requires a bulk string (e.g. an array passed as a key).
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
                    b"ping" => {
                        let Some(message_frame) = iter.next() else {
                            return Ok(Command::ping(None));
                        };
                        let Frame::BulkString(message) = message_frame else {
                            return Err(CommandFromFrameError::UnexpectedFrame);
                        };
                        if iter.next().is_some() {
                            return Err(CommandError::TooManyParts.into());
                        }
                        Ok(Command::ping(Some(message)))
                    }
                    b"exists" => {
                        let key = iter.next().ok_or(CommandError::NotEnoughParts)?;
                        let Frame::BulkString(key) = key else {
                            return Err(CommandFromFrameError::UnexpectedFrame);
                        };
                        if iter.next().is_some() {
                            return Err(CommandError::TooManyParts.into());
                        }
                        Ok(Self::exists(key))
                    }
                    b"expire" => {
                        let (Some(key), Some(ttl)) = (iter.next(), iter.next()) else {
                            return Err(CommandError::NotEnoughParts.into());
                        };
                        let (Frame::BulkString(key), Frame::BulkString(ttl_bytes)) = (key, ttl)
                        else {
                            return Err(CommandFromFrameError::UnexpectedFrame);
                        };
                        let ttl = std::str::from_utf8(&ttl_bytes)
                            .map_err(CommandError::from)?
                            .parse()
                            .map_err(CommandError::from)?;
                        Ok(Self::expire(key, ttl))
                    }
                    b"expireat" => {
                        let (Some(key), Some(ttl)) = (iter.next(), iter.next()) else {
                            return Err(CommandError::NotEnoughParts.into());
                        };
                        let (Frame::BulkString(key), Frame::BulkString(ttl_bytes)) = (key, ttl)
                        else {
                            return Err(CommandFromFrameError::UnexpectedFrame);
                        };
                        let ttl = std::str::from_utf8(&ttl_bytes)
                            .map_err(CommandError::from)?
                            .parse()
                            .map_err(CommandError::from)?;
                        Ok(Self::expire_at(key, ttl))
                    }
                    b"ttl" => {
                        let key = iter.next().ok_or(CommandError::NotEnoughParts)?;
                        let Frame::BulkString(key) = key else {
                            return Err(CommandFromFrameError::UnexpectedFrame);
                        };
                        if iter.next().is_some() {
                            return Err(CommandError::TooManyParts.into());
                        }
                        Ok(Command::ttl(key))
                    }
                    b"persist" => {
                        let key = iter.next().ok_or(CommandError::NotEnoughParts)?;
                        let Frame::BulkString(key) = key else {
                            return Err(CommandFromFrameError::UnexpectedFrame);
                        };
                        if iter.next().is_some() {
                            return Err(CommandError::TooManyParts.into());
                        }
                        Ok(Command::persist(key))
                    }
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

    // ---------- ok cases ----------

    #[test]
    fn try_from_frame_ok_get() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"get".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ]))
            .unwrap(),
            Command::get(b"foo")
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"GET".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ]))
            .unwrap(),
            Command::get(b"foo")
        );
    }

    #[test]
    fn try_from_frame_ok_set() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"set".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"value".to_vec()),
            ]))
            .unwrap(),
            Command::set(b"foo", b"value")
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"SET".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"value".to_vec()),
            ]))
            .unwrap(),
            Command::set(b"foo", b"value")
        );
    }

    #[test]
    fn try_from_frame_ok_del() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"del".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ]))
            .unwrap(),
            Command::delete(b"foo")
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"DEL".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ]))
            .unwrap(),
            Command::delete(b"foo")
        );
    }

    #[test]
    fn try_from_frame_ok_ping_without_message() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"ping".to_vec())])).unwrap(),
            Command::ping(None)
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"PING".to_vec())])).unwrap(),
            Command::ping(None)
        );
    }

    #[test]
    fn try_from_frame_ok_ping_with_message() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"ping".to_vec()),
                Frame::BulkString(b"hello".to_vec()),
            ]))
            .unwrap(),
            Command::ping(Some(b"hello".to_vec()))
        );
    }

    #[test]
    fn try_from_frame_ok_exists() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"exists".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ]))
            .unwrap(),
            Command::exists(b"foo")
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"EXISTS".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ]))
            .unwrap(),
            Command::exists(b"foo")
        );
    }

    #[test]
    fn try_from_frame_ok_expire() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expire".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"123".to_vec()),
            ]))
            .unwrap(),
            Command::expire("foo", 123),
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"EXPIRE".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"123".to_vec()),
            ]))
            .unwrap(),
            Command::expire("foo", 123),
        );
    }

    #[test]
    fn try_from_frame_ok_expireat() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expireat".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"1700000000".to_vec()),
            ]))
            .unwrap(),
            Command::expire_at("foo", 1_700_000_000),
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"EXPIREAT".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"1700000000".to_vec()),
            ]))
            .unwrap(),
            Command::expire_at("foo", 1_700_000_000),
        );
    }

    #[test]
    fn try_from_frame_ok_ttl() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"ttl".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ]))
            .unwrap(),
            Command::ttl("foo"),
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"TTL".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ]))
            .unwrap(),
            Command::ttl("foo"),
        );
    }

    #[test]
    fn try_from_frame_ok_persist() {
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"persist".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ]))
            .unwrap(),
            Command::persist("foo"),
        );
        assert_eq!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"PERSIST".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ]))
            .unwrap(),
            Command::persist("foo"),
        );
    }

    // ---------- top-level shape errors ----------

    #[test]
    fn try_from_frame_err_empty_array() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_command_value_is_array() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::Array(vec![Frame::BulkString(
                b"get".to_vec()
            )])])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    #[test]
    fn try_from_frame_err_unrecognized_command() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"foo".to_vec())])),
            Err(CommandFromFrameError::CommandError(
                CommandError::UnrecognizedCommand
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_unexpected_value() {
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

    // ---------- GET — errors ----------

    #[test]
    fn try_from_frame_err_get_too_many_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"get".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::TooManyParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_get_not_enough_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"get".to_vec()),])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_get_unexpected_frame_key() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"get".to_vec()),
                Frame::Array(vec![]),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    // ---------- SET — errors ----------

    #[test]
    fn try_from_frame_err_set_too_many_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"set".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"value".to_vec()),
                Frame::BulkString(b"value".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::TooManyParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_set_not_enough_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"set".to_vec()),])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"set".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_set_unexpected_frame_key() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"set".to_vec()),
                Frame::Array(vec![]),
                Frame::BulkString(b"value".to_vec()),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    #[test]
    fn try_from_frame_err_set_unexpected_frame_value() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"set".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::Array(vec![]),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    // ---------- DEL — errors ----------

    #[test]
    fn try_from_frame_err_del_too_many_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"del".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::TooManyParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_del_not_enough_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"del".to_vec()),])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_del_unexpected_frame_key() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"del".to_vec()),
                Frame::Array(vec![]),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    // ---------- EXISTS — errors ----------

    #[test]
    fn try_from_frame_err_exists_too_many_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"exists".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::TooManyParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_exists_not_enough_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"exists".to_vec())])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_exists_unexpected_frame_key() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"exists".to_vec()),
                Frame::Array(vec![]),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    // ---------- PING — errors ----------

    #[test]
    fn try_from_frame_err_ping_too_many_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"ping".to_vec()),
                Frame::BulkString(b"hello".to_vec()),
                Frame::BulkString(b"world".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::TooManyParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_ping_unexpected_frame_message() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"ping".to_vec()),
                Frame::Array(vec![]),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    // ---------- EXPIRE — errors ----------

    #[test]
    fn try_from_frame_err_expire_not_enough_parts_zero_args() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"expire".to_vec())])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_expire_not_enough_parts_one_arg() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expire".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_expire_unexpected_frame_key() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expire".to_vec()),
                Frame::Array(vec![]),
                Frame::BulkString(b"123".to_vec()),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    #[test]
    fn try_from_frame_err_expire_unexpected_frame_ttl() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expire".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::Array(vec![]),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    #[test]
    fn try_from_frame_err_expire_ttl_invalid_utf8() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expire".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(vec![0xff, 0xfe]),
            ])),
            Err(CommandFromFrameError::CommandError(CommandError::Utf8(_)))
        ));
    }

    #[test]
    fn try_from_frame_err_expire_ttl_not_a_number() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expire".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"abc".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(CommandError::ParseInt(
                _
            )))
        ));
    }

    #[test]
    fn try_from_frame_err_expire_ttl_negative() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expire".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"-1".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(CommandError::ParseInt(
                _
            )))
        ));
    }

    // ---------- EXPIREAT — errors ----------

    #[test]
    fn try_from_frame_err_expireat_not_enough_parts_zero_args() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"expireat".to_vec())])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_expireat_not_enough_parts_one_arg() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expireat".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_expireat_unexpected_frame_key() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expireat".to_vec()),
                Frame::Array(vec![]),
                Frame::BulkString(b"1700000000".to_vec()),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    #[test]
    fn try_from_frame_err_expireat_unexpected_frame_ttl() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expireat".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::Array(vec![]),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    #[test]
    fn try_from_frame_err_expireat_ttl_invalid_utf8() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expireat".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(vec![0xff, 0xfe]),
            ])),
            Err(CommandFromFrameError::CommandError(CommandError::Utf8(_)))
        ));
    }

    #[test]
    fn try_from_frame_err_expireat_ttl_not_a_number() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expireat".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"abc".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(CommandError::ParseInt(
                _
            )))
        ));
    }

    #[test]
    fn try_from_frame_err_expireat_ttl_negative() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"expireat".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"-1".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(CommandError::ParseInt(
                _
            )))
        ));
    }

    // ---------- TTL — errors ----------

    #[test]
    fn try_from_frame_err_ttl_too_many_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"ttl".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"bar".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::TooManyParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_ttl_not_enough_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"ttl".to_vec())])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_ttl_unexpected_frame_key() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"ttl".to_vec()),
                Frame::Array(vec![]),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }

    // ---------- PERSIST — errors ----------

    #[test]
    fn try_from_frame_err_persist_too_many_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"persist".to_vec()),
                Frame::BulkString(b"foo".to_vec()),
                Frame::BulkString(b"bar".to_vec()),
            ])),
            Err(CommandFromFrameError::CommandError(
                CommandError::TooManyParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_persist_not_enough_parts() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![Frame::BulkString(b"persist".to_vec())])),
            Err(CommandFromFrameError::CommandError(
                CommandError::NotEnoughParts
            ))
        ));
    }

    #[test]
    fn try_from_frame_err_persist_unexpected_frame_key() {
        assert!(matches!(
            Command::try_from(Frame::Array(vec![
                Frame::BulkString(b"persist".to_vec()),
                Frame::Array(vec![]),
            ])),
            Err(CommandFromFrameError::UnexpectedFrame)
        ));
    }
}
