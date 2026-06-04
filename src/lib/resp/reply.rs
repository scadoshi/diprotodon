//! Outbound RESP — the [`Reply`] enum the server emits and the [`SimpleInner`] newtype
//! that guards RESP's "no CR/LF in simple frames" invariant at construction time.

use std::io::Write;
use thiserror::Error;

/// Errors returned by [`SimpleInner::try_from`] when payload bytes contain a
/// forbidden character. Simple-string and simple-error frames must not carry `\r` or
/// `\n` because RESP uses those bytes as the frame terminator.
#[derive(Debug, Error)]
pub enum SimpleInnerError {
    #[error("must not contain a carriage return: (\"\\r\")")]
    IncludesCarriageReturn,
    #[error("must not contain a line feed (\"\\n\")")]
    IncludesLineFeed,
}

/// Validated payload for a RESP simple string or simple error.
///
/// Construction enforces the no-CR/LF rule, so once a `SimpleInner` exists it's
/// guaranteed safe to write between a sigil byte and a `\r\n` terminator. The serializer
/// in [`Reply::write_to`] relies on that.
///
/// Three constructors are exposed:
///
/// - [`SimpleInner::ok`] / [`SimpleInner::pong`] — trusted constants used in normal replies.
/// - [`SimpleInner::sanitized`] — for arbitrary error message bytes; strips `\r`/`\n`
///   defensively instead of returning a `Result` (errors crossing this boundary should
///   never themselves be a source of new errors).
#[derive(Debug, Clone, PartialEq)]
pub struct SimpleInner(Vec<u8>);

impl TryFrom<&[u8]> for SimpleInner {
    type Error = SimpleInnerError;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.contains(&b'\r') {
            return Err(SimpleInnerError::IncludesCarriageReturn);
        }
        if value.contains(&b'\n') {
            return Err(SimpleInnerError::IncludesLineFeed);
        }
        Ok(Self(value.to_vec()))
    }
}

impl SimpleInner {
    /// Borrow the validated payload bytes for serialization.
    fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Trusted constructor for the canonical `+OK\r\n` reply payload.
    pub fn ok() -> Self {
        Self(b"OK".to_vec())
    }

    /// Trusted constructor for the canonical `+PONG\r\n` reply payload.
    pub fn pong() -> Self {
        Self(b"PONG".to_vec())
    }

    /// Strip any `\r` or `\n` bytes from `bytes` and wrap the result. Use this when
    /// the payload comes from a `Display`-formatted error or any other source where
    /// CR/LF is plausible — guarantees a valid frame without forcing the caller to
    /// handle a `Result`.
    pub fn sanitized(bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = bytes
            .into()
            .into_iter()
            .filter(|b| *b != b'\r' && *b != b'\n')
            .collect();
        Self(bytes)
    }
}

/// A RESP reply the server can write back to a client. Variants cover every reply
/// shape this server emits today.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// `+<payload>\r\n` — e.g. `+OK` or `+PONG`.
    SimpleString(SimpleInner),
    /// `-<payload>\r\n` — e.g. `-ERR unknown command`.
    SimpleError(SimpleInner),
    /// `$<len>\r\n<bytes>\r\n` — binary-safe value reply (e.g. GET hit, PING with message).
    BulkString(Vec<u8>),
    /// `$-1\r\n` — the RESP "no such key" sentinel returned by GET on a miss.
    NullBulk,
    /// `:<n>\r\n` — used for boolean-as-int replies (EXISTS, DEL, EXPIRE, PERSIST) and
    /// for TTL's `-2`/`-1`/`n` ladder.
    Integer(i64),
}

impl Reply {
    /// Serialize this reply onto `w` as RESP bytes. Uses `write_all` so partial writes
    /// don't leave a half-formed frame on the wire; the caller is expected to flush
    /// after dispatching a reply (the session does this).
    pub fn write_to(&self, w: &mut impl Write) -> std::io::Result<()> {
        match self {
            Reply::SimpleString(inner) => {
                w.write_all(b"+")?;
                w.write_all(inner.as_bytes())?;
                w.write_all(b"\r\n")?;
            }
            Reply::SimpleError(inner) => {
                w.write_all(b"-")?;
                w.write_all(inner.as_bytes())?;
                w.write_all(b"\r\n")?;
            }
            Reply::BulkString(bytes) => {
                w.write_all(b"$")?;
                w.write_all(bytes.len().to_string().as_bytes())?;
                w.write_all(b"\r\n")?;
                w.write_all(bytes.as_slice())?;
                w.write_all(b"\r\n")?;
            }
            Reply::NullBulk => {
                w.write_all(b"$-1\r\n")?;
            }
            Reply::Integer(int) => {
                w.write_all(b":")?;
                w.write_all(int.to_string().as_bytes())?;
                w.write_all(b"\r\n")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simple_inner_from_bytes_ok() {
        assert_eq!(
            SimpleInner::try_from(b"foo".as_slice()).unwrap(),
            SimpleInner(b"foo".to_vec())
        );
    }
    #[test]
    fn simple_inner_from_bytes_err_cr() {
        assert!(matches!(
            SimpleInner::try_from(b"foo\r".as_slice()),
            Err(SimpleInnerError::IncludesCarriageReturn)
        ));
    }
    #[test]
    fn simple_inner_from_bytes_err_lf() {
        assert!(matches!(
            SimpleInner::try_from(b"foo\n".as_slice()),
            Err(SimpleInnerError::IncludesLineFeed)
        ));
    }
    #[test]
    fn write_to_ok_simple_string() {
        let mut buf = Vec::new();
        Reply::SimpleString(SimpleInner::try_from(b"foo".as_slice()).unwrap())
            .write_to(&mut buf)
            .unwrap();
        assert_eq!(buf, b"+foo\r\n");
    }
    #[test]
    fn write_to_ok_simple_error() {
        let mut buf = Vec::new();
        Reply::SimpleError(SimpleInner::try_from(b"foo".as_slice()).unwrap())
            .write_to(&mut buf)
            .unwrap();
        assert_eq!(buf, b"-foo\r\n");
    }
    #[test]
    fn write_to_ok_bulk_string() {
        let mut buf = Vec::new();
        Reply::BulkString(b"foo\r\nbar".to_vec())
            .write_to(&mut buf)
            .unwrap();
        assert_eq!(buf, b"$8\r\nfoo\r\nbar\r\n");
    }
    #[test]
    fn write_to_ok_bulk_string_empty() {
        let mut buf = Vec::new();
        Reply::BulkString(b"".to_vec()).write_to(&mut buf).unwrap();
        assert_eq!(buf, b"$0\r\n\r\n");
    }
    #[test]
    fn write_to_ok_null_bulk() {
        let mut buf = Vec::new();
        Reply::NullBulk.write_to(&mut buf).unwrap();
        assert_eq!(buf, b"$-1\r\n");
    }
    #[test]
    fn write_to_ok_integer() {
        let mut buf = Vec::new();
        Reply::Integer(123).write_to(&mut buf).unwrap();
        assert_eq!(buf, b":123\r\n");
    }
    #[test]
    fn write_to_ok_integer_negative() {
        let mut buf = Vec::new();
        Reply::Integer(-123).write_to(&mut buf).unwrap();
        assert_eq!(buf, b":-123\r\n");
    }
}
