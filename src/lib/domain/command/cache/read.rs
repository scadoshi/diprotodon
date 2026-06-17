#[derive(Debug, Clone, PartialEq)]
pub enum ReadCommand {
    Get { key: Vec<u8> },
    Exists { key: Vec<u8> },
    Ttl { key: Vec<u8> },
}

impl ReadCommand {
    pub fn get(key: impl Into<Vec<u8>>) -> Self {
        Self::Get { key: key.into() }
    }

    pub fn exists(key: impl Into<Vec<u8>>) -> Self {
        Self::Exists { key: key.into() }
    }

    pub fn ttl(key: impl Into<Vec<u8>>) -> Self {
        Self::Ttl { key: key.into() }
    }
}
