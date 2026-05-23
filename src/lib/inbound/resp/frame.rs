use crate::inbound::resp::crlf::Crlf;
use std::{num::ParseIntError, str::Utf8Error};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FrameError {
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

#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Array(Vec<Frame>),
    BulkString(Vec<u8>),
}

impl Frame {
    pub fn parse_one(bytes: &[u8]) -> Result<(Frame, &[u8]), FrameError> {
        let Some((header, bytes)) = bytes.split_crlf() else {
            return Err(FrameError::Incomplete);
        };
        if header.len() < 2 {
            return Err(FrameError::Malformed);
        }
        let len = std::str::from_utf8(&header[1..])
            .map_err(ParseLengthError::from)?
            .parse::<usize>()
            .map_err(ParseLengthError::from)?;
        if header[0] == b'*' {
            Frame::parse_array(bytes, len)
        } else if header[0] == b'$' {
            Frame::parse_bulk_string(bytes, len)
        } else {
            Err(FrameError::UnknownSigil)
        }
    }

    pub fn parse_array(bytes: &[u8], len: usize) -> Result<(Frame, &[u8]), FrameError> {
        let mut vec = Vec::new();
        let mut buf: &[u8] = bytes;
        for _ in 0..len {
            let (value, bytes) = Frame::parse_one(buf)?;
            vec.push(value);
            buf = bytes;
        }
        Ok((Frame::Array(vec), buf))
    }

    pub fn parse_bulk_string(bytes: &[u8], len: usize) -> Result<(Frame, &[u8]), FrameError> {
        if bytes.len() < len + 2 || &bytes[len..len + 2] != b"\r\n" {
            return Err(FrameError::MissingTerminator);
        }
        Ok((Frame::BulkString(bytes[0..len].to_vec()), &bytes[len + 2..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_bulk_string_ok_basic() {
        assert_eq!(
            Frame::parse_bulk_string(b"foo\r\n", 3).unwrap(),
            (Frame::BulkString(b"foo".to_vec()), "".as_bytes())
        );
    }
    #[test]
    fn parse_bulk_string_ok_with_inner_terminator() {
        assert_eq!(
            Frame::parse_bulk_string(b"foo\r\nbar\r\n", 8).unwrap(),
            (Frame::BulkString(b"foo\r\nbar".to_vec()), "".as_bytes())
        );
    }
    #[test]
    fn parse_bulk_string_err_missing_terminator() {
        assert!(matches!(
            Frame::parse_bulk_string(b"foo", 1),
            Err(FrameError::MissingTerminator)
        ));
    }
    #[test]
    fn parse_array_ok_basic() {
        assert_eq!(
            Frame::parse_array(b"$3\r\nfoo\r\n$3\r\nbar\r\nfoo", 2).unwrap(),
            (
                Frame::Array(vec![
                    Frame::BulkString(b"foo".to_vec()),
                    Frame::BulkString(b"bar".to_vec()),
                ]),
                "foo".as_bytes()
            )
        );
    }
    #[test]
    fn parse_array_ok_empty() {
        assert_eq!(
            Frame::parse_array(b"", 0).unwrap(),
            (Frame::Array(vec![]), "".as_bytes())
        )
    }
    #[test]
    fn parse_one_ok_basic_bulk_string() {
        assert_eq!(
            Frame::parse_one(b"$3\r\nfoo\r\nbar").unwrap(),
            (Frame::BulkString(b"foo".to_vec()), "bar".as_bytes(),)
        );
    }
    #[test]
    fn parse_one_ok_basic_array() {
        assert_eq!(
            Frame::parse_one(b"*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\nfoo").unwrap(),
            (
                Frame::Array(vec![
                    Frame::BulkString(b"foo".to_vec()),
                    Frame::BulkString(b"bar".to_vec())
                ]),
                "foo".as_bytes(),
            )
        );
    }
    #[test]
    fn parse_one_ok_empty_array() {
        assert_eq!(
            Frame::parse_one(b"*0\r\n").unwrap(),
            (Frame::Array(vec![]), "".as_bytes())
        )
    }
    #[test]
    fn parse_one_ok_nested_array() {
        assert_eq!(
            Frame::parse_one(b"*1\r\n*2\r\n$3\r\nfoo\r\n$3\r\nbar\r\nbaz").unwrap(),
            (
                Frame::Array(vec![Frame::Array(vec![
                    Frame::BulkString(b"foo".to_vec()),
                    Frame::BulkString(b"bar".to_vec())
                ])]),
                "baz".as_bytes()
            )
        )
    }
    #[test]
    fn parse_one_err_incomplete() {
        assert!(matches!(
            Frame::parse_one(b"foo"),
            Err(FrameError::Incomplete)
        ));
        assert!(matches!(Frame::parse_one(b""), Err(FrameError::Incomplete)));
    }
    #[test]
    fn parse_one_err_malformed() {
        assert!(matches!(
            Frame::parse_one(b"$\r\n"),
            Err(FrameError::Malformed)
        ));
    }
    #[test]
    fn parse_one_err_invalid_length() {
        assert!(matches!(
            Frame::parse_one(b"$\xFF\xFE\r\nbar"),
            Err(FrameError::InvalidLength(ParseLengthError::Utf8(_)))
        ));
        assert!(matches!(
            Frame::parse_one(b"$foo\r\nbar"),
            Err(FrameError::InvalidLength(ParseLengthError::ParseInt(_)))
        ));
    }
    #[test]
    fn parse_one_err_invalid_unknown_sigil() {
        assert!(matches!(
            Frame::parse_one(b"?2\r\n"),
            Err(FrameError::UnknownSigil)
        ));
    }
    #[test]
    fn parse_one_err_invalid_missing_terminator() {
        assert!(matches!(
            Frame::parse_one(b"$3\r\nfoo"),
            Err(FrameError::MissingTerminator)
        ));
    }
}
