//! Inbound layer — everything that turns incoming bytes into a [`crate::domain::command::Command`].
//!
//! - [`server`] — TCP accept loop, thread-per-connection, sweeper/persist/shutdown threads.
//! - [`session`] — the per-connection REPL: reads bytes, parses frames, dispatches commands.

use crate::{
    domain::command::{CommandOutcome, TtlOutcome},
    resp::reply::{Reply, SimpleInner},
};

pub mod server;
pub mod session;

type CO = CommandOutcome;
impl From<CommandOutcome> for Reply {
    fn from(value: CommandOutcome) -> Self {
        match value {
            CO::Value(Some(value)) => Self::BulkString(value),
            CO::Value(None) => Self::NullBulk,
            CO::Ok => Self::SimpleString(SimpleInner::ok()),
            CO::Pong(Some(message)) => Self::SimpleString(SimpleInner::sanitized(message)),
            CO::Pong(None) => Self::SimpleString(SimpleInner::pong()),
            CO::Bool(bool) => Self::Integer(bool as i64),
            CO::Ttl(TtlOutcome::KeyNotFound) => Self::Integer(-2),
            CO::Ttl(TtlOutcome::TtlNotFound) => Self::Integer(-1),
            CO::Ttl(TtlOutcome::Some(ttl)) => Self::Integer(i64::try_from(ttl).unwrap_or(i64::MAX)),
            CO::Integer(int) => Self::Integer(int),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_some_maps_to_bulk_string() {
        assert_eq!(
            Reply::from(CO::Value(Some(b"foo".to_vec()))),
            Reply::BulkString(b"foo".to_vec())
        );
    }

    #[test]
    fn value_none_maps_to_null_bulk() {
        assert_eq!(Reply::from(CO::Value(None)), Reply::NullBulk);
    }

    #[test]
    fn ok_maps_to_simple_string_ok() {
        assert_eq!(Reply::from(CO::Ok), Reply::SimpleString(SimpleInner::ok()));
    }

    #[test]
    fn pong_none_maps_to_simple_string_pong() {
        assert_eq!(
            Reply::from(CO::Pong(None)),
            Reply::SimpleString(SimpleInner::pong())
        );
    }

    #[test]
    fn pong_some_maps_to_simple_string_sanitized() {
        assert_eq!(
            Reply::from(CO::Pong(Some(b"hi".to_vec()))),
            Reply::SimpleString(SimpleInner::sanitized(b"hi".to_vec()))
        );
    }

    #[test]
    fn pong_some_with_crlf_is_sanitized() {
        assert_eq!(
            Reply::from(CO::Pong(Some(b"hi\r\nthere".to_vec()))),
            Reply::SimpleString(SimpleInner::sanitized(b"hi\r\nthere".to_vec()))
        );
    }

    #[test]
    fn bool_true_maps_to_integer_one() {
        assert_eq!(Reply::from(CO::Bool(true)), Reply::Integer(1));
    }

    #[test]
    fn bool_false_maps_to_integer_zero() {
        assert_eq!(Reply::from(CO::Bool(false)), Reply::Integer(0));
    }

    #[test]
    fn ttl_key_not_found_maps_to_integer_negative_two() {
        assert_eq!(
            Reply::from(CO::Ttl(TtlOutcome::KeyNotFound)),
            Reply::Integer(-2)
        );
    }

    #[test]
    fn ttl_ttl_not_found_maps_to_integer_negative_one() {
        assert_eq!(
            Reply::from(CO::Ttl(TtlOutcome::TtlNotFound)),
            Reply::Integer(-1)
        );
    }

    #[test]
    fn ttl_some_maps_to_integer() {
        assert_eq!(
            Reply::from(CO::Ttl(TtlOutcome::Some(123))),
            Reply::Integer(123)
        );
    }

    #[test]
    fn ttl_some_overflow_saturates_to_i64_max() {
        assert_eq!(
            Reply::from(CO::Ttl(TtlOutcome::Some(u64::MAX))),
            Reply::Integer(i64::MAX)
        );
    }

    #[test]
    fn integer_passes_through() {
        assert_eq!(Reply::from(CO::Integer(42)), Reply::Integer(42));
        assert_eq!(Reply::from(CO::Integer(-7)), Reply::Integer(-7));
    }
}
