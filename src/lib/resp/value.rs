use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValueError {
    #[error("unrecognized type")]
    UnrecognizedType,
    #[error("incomplete frame")]
    IncompleteFrame,
    #[error("invalid type for given type")]
    InvalidShape,
    #[error("empty bytes")]
    EmptyBytes,
}

#[derive(Debug)]
pub enum Value {
    Array(Vec<Value>),
    BulkString(Vec<u8>),
    // not supporting other types yet
}

impl TryFrom<&[u8]> for Value {
    type Error = ValueError;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let Some(first) = value.first() else {
            return Err(ValueError::EmptyBytes);
        };
        if *first == b'*' {
            todo!()
        } else if *first == b'$' {
            todo!()
        } else {
            Err(ValueError::UnrecognizedType)
        }
    }
}
