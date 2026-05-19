use crate::resp::value::ValueError;

pub struct Decoder;

pub enum DecoderResult {
    Ok(String),
    Err(ValueError),
    IncompleteFrame,
}

impl Decoder {
    pub fn decode(_bytes: &[u8]) -> DecoderResult {
        todo!()
    }
}
