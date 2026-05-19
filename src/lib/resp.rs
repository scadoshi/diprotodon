use thiserror::Error;

trait RespBytes {
    fn is_crlf(&self) -> bool;
    fn next_value(&self) -> Option<(&[u8], &[u8])>;
}

impl RespBytes for [u8] {
    fn is_crlf(&self) -> bool {
        if let (Some(first_byte), Some(second_byte)) = (self.first(), self.get(1)) {
            *first_byte == b'\r' && *second_byte == b'\n'
        } else {
            false
        }
    }
    fn next_value(&self) -> Option<(&[u8], &[u8])> {
        if self.is_crlf() {
            return None;
        }
        let mut p = 0;
        while p < self.len() && !&self[p..].is_crlf() {
            p += 1;
        }
        Some((&self[..p], &self[p..]))
    }
}

pub struct Decoder;

pub enum DecoderResult {
    Ok(String),
    Err(ValueError),
    IncompleteFrame,
}

impl Decoder {
    pub fn decode(bytes: &[u8]) -> DecoderResult {
        todo!()
    }
}

#[derive(Debug, Error)]
pub enum ValueError {
    #[error("unrecognized type")]
    UnrecognizedType,
    #[error("incomplete frame")]
    IncompleteFrame,
    #[error("invalid type for given type")]
    InvalidShape,
}
