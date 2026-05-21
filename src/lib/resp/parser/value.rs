use crate::resp::parser::crlf::Crlf;
use std::{num::ParseIntError, str::Utf8Error};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValueError {
    #[error("missing crlf terminator")]
    MissingTerminator,
    #[error("unknown sigil")]
    UnknownSigil,
    #[error("failed to parse length: {0}")]
    InvalidLength(#[from] ParseLengthError),
    #[error("incomplete frame")]
    Incomplete,
    #[error("malformed value")]
    Malformed,
}

#[derive(Debug, Error)]
pub enum ParseLengthError {
    #[error(transparent)]
    Utf8(#[from] Utf8Error),
    #[error(transparent)]
    ParseInt(#[from] ParseIntError),
}

#[derive(Debug, PartialEq)]
pub enum Value {
    Array(Vec<Value>),
    BulkString(Vec<u8>),
    // not supporting other types yet
}

impl Value {
    fn parse_one(bytes: &[u8]) -> Result<(Value, &[u8]), ValueError> {
        let Some((header, bytes)) = bytes.split_crlf() else {
            return Err(ValueError::Incomplete);
        };
        if header.len() < 2 {
            return Err(ValueError::Malformed);
        }
        let len = std::str::from_utf8(&header[1..])
            .map_err(ParseLengthError::from)?
            .parse::<usize>()
            .map_err(ParseLengthError::from)?;
        if header[0] == b'*' {
            Value::parse_array(bytes, len)
        } else if header[0] == b'$' {
            Value::parse_bulk_string(bytes, len)
        } else {
            Err(ValueError::UnknownSigil)
        }
    }

    fn parse_array(bytes: &[u8], len: usize) -> Result<(Value, &[u8]), ValueError> {
        let mut vec = Vec::new();
        let mut buf: &[u8] = bytes;
        for _ in 0..len {
            let (value, bytes) = Value::parse_one(buf)?;
            vec.push(value);
            buf = bytes;
        }
        Ok((Value::Array(vec), buf))
    }

    fn parse_bulk_string(bytes: &[u8], len: usize) -> Result<(Value, &[u8]), ValueError> {
        if bytes.len() < len + 2 || &bytes[len..len + 2] != b"\r\n" {
            return Err(ValueError::MissingTerminator);
        }
        Ok((Value::BulkString(bytes[0..len].to_vec()), &bytes[len + 2..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_bulk_string_ok_basic() {
        assert_eq!(
            Value::parse_bulk_string(b"a\r\n", 1).unwrap(),
            (Value::BulkString(b"a".to_vec()), "".as_bytes())
        );
    }
    #[test]
    fn parse_bulk_string_ok_with_inner_terminator() {
        assert_eq!(
            Value::parse_bulk_string(b"foo\r\nbar\r\n", 8).unwrap(),
            (Value::BulkString(b"foo\r\nbar".to_vec()), "".as_bytes())
        );
    }
    #[test]
    fn parse_bulk_string_err_missing_terminator() {
        assert!(matches!(
            Value::parse_bulk_string(b"a", 1),
            Err(ValueError::MissingTerminator)
        ));
    }
    #[test]
    fn parse_array_ok_basic() {
        assert_eq!(
            Value::parse_array(b"$3\r\nfoo\r\n$3\r\nbar\r\nfoo", 2).unwrap(),
            (
                Value::Array(vec![
                    Value::BulkString(b"foo".to_vec()),
                    Value::BulkString(b"bar".to_vec()),
                ]),
                "foo".as_bytes()
            )
        );
    }
    #[test]
    fn parse_array_ok_empty() {
        assert_eq!(
            Value::parse_array(b"", 0).unwrap(),
            (Value::Array(vec![]), "".as_bytes())
        )
    }
    #[test]
    fn parse_one_ok_basic_bulk_string() {
        assert_eq!(
            Value::parse_one(b"$3\r\nfoo\r\nbar").unwrap(),
            (Value::BulkString(b"foo".to_vec()), "bar".as_bytes(),)
        );
    }
    #[test]
    fn parse_one_ok_basic_array() {
        assert_eq!(
            Value::parse_one(b"*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\nfoo").unwrap(),
            (
                Value::Array(vec![
                    Value::BulkString(b"foo".to_vec()),
                    Value::BulkString(b"bar".to_vec())
                ]),
                "foo".as_bytes(),
            )
        );
    }
    #[test]
    fn parse_one_ok_empty_array() {
        assert_eq!(
            Value::parse_one(b"*0\r\n").unwrap(),
            (Value::Array(vec![]), "".as_bytes())
        )
    }
    #[test]
    fn parse_one_ok_nested_array() {
        assert_eq!(
            Value::parse_one(b"*1\r\n*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\nbaz").unwrap(),
            (
                Value::Array(vec![Value::Array(vec![
                    Value::BulkString(b"foo".to_vec()),
                    Value::BulkString(b"bar".to_vec())
                ])]),
                "baz".as_bytes()
            )
        )
    }
    #[test]
    fn parse_one_err_incomplete() {
        assert!(matches!(
            Value::parse_one(b"foo"),
            Err(ValueError::Incomplete)
        ));
        assert!(matches!(Value::parse_one(b""), Err(ValueError::Incomplete)));
    }
    #[test]
    fn parse_one_err_malformed() {
        assert!(matches!(
            Value::parse_one(b"$\r\n"),
            Err(ValueError::Malformed)
        ));
    }
    #[test]
    fn parse_one_err_invalid_length() {
        assert!(matches!(
            Value::parse_one(b"$\xFF\xFE\r\nbar"),
            Err(ValueError::InvalidLength(ParseLengthError::Utf8(_)))
        ));
        assert!(matches!(
            Value::parse_one(b"$foo\r\nbar"),
            Err(ValueError::InvalidLength(ParseLengthError::ParseInt(_)))
        ));
    }
    #[test]
    fn parse_one_err_invalid_unknown_sigil() {
        assert!(matches!(
            Value::parse_one(b"?2\r\n"),
            Err(ValueError::UnknownSigil)
        ));
    }
    #[test]
    fn parse_one_err_invalid_missing_terminator() {
        assert!(matches!(
            Value::parse_one(b"$3\r\nfoo"),
            Err(ValueError::MissingTerminator)
        ));
    }
}
