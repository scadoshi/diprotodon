use crate::resp::utils::Crlf;
use std::{num::ParseIntError, str::Utf8Error};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValueError {
    #[error("missing crlf")]
    MissingCrlf,
    #[error("unrecognized type")]
    UnrecognizedType,
    #[error("failed to parse length: {0}")]
    ParseLength(#[from] ParseLengthError),
    #[error("given bytes: {0:?} do not match given len: {1}")]
    BytesLenMismatch(Vec<u8>, usize),
}

#[derive(Debug, Error)]
pub enum ParseLengthError {
    #[error(transparent)]
    Utf8(#[from] Utf8Error),
    #[error(transparent)]
    ParseInt(#[from] ParseIntError),
}

#[derive(Debug)]
pub enum Value {
    Array(Vec<Value>),
    BulkString(Vec<u8>),
    // not supporting other types yet
}

impl Value {
    fn parse_one(bytes: &[u8]) -> Result<(Value, &[u8]), ValueError> {
        // Example
        // *2\r\n$3\r\nGET\r\n$3\r\nfoo\r\n
        // Should result in something like
        // Value::Array(vec![Value::BulkString(b"GET".to_vec()), Value::BulkString(b"foo".to_vec()])
        let Some((sigil_len_str, leftovers)) = bytes.split_crlf() else {
            return Err(ValueError::MissingCrlf);
        };
        let Some(sigil) = sigil_len_str.first() else {
            return Err(ValueError::UnrecognizedType);
        };
        let len = std::str::from_utf8(&sigil_len_str[1..])
            .map_err(ParseLengthError::from)?
            .parse::<usize>()
            .map_err(ParseLengthError::from)?;
        if *sigil == b'*' {
            // array path
        } else if *sigil == b'$' {
            // bulk string path
        }
        todo!()
    }

    fn parse_array(bytes: &[u8], len: u32) -> Result<Value, ValueError> {
        todo!()
    }

    fn parse_bulk_string(bytes: &[u8], len: usize) -> Result<(Value, &[u8]), ValueError> {
        // len: 4
        // ab\r\n\r\n
        // should return `Ok(Value::BulkString(b"ab\r\n".to_vec()))`
        if bytes.len() < len + 2 {
            return Err(ValueError::BytesLenMismatch(bytes.to_vec(), len));
        }
        Ok((Value::BulkString(bytes[0..len].to_vec()), bytes))
    }
}

impl TryFrom<&[u8]> for Value {
    type Error = ValueError;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        todo!()
    }
}
