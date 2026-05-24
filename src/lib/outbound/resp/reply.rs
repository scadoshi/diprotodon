use std::io::Write;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SimpleInnerError {
    #[error("must not contain a carriage return: (\"\\r\")")]
    IncludesCarriageReturn,
    #[error("must not contain a line feed (\"\\n\")")]
    IncludesLineFeed,
}

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
    fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn ok() -> Self {
        Self(b"OK".to_vec())
    }

    pub fn pong() -> Self {
        Self(b"PONG".to_vec())
    }

    pub fn sanitized(bytes: impl Into<Vec<u8>>) -> Self {
        let bytes = bytes
            .into()
            .into_iter()
            .filter(|b| *b != b'\r' && *b != b'\n')
            .collect();
        Self(bytes)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    SimpleString(SimpleInner),
    SimpleError(SimpleInner),
    BulkString(Vec<u8>),
    NullBulk,
    Integer(i64),
}

impl Reply {
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
