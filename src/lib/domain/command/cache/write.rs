#[derive(Debug, Clone, PartialEq)]
pub enum WriteCommand {
    Set { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
    Expire { key: Vec<u8>, relative_ttl: u64 },
    ExpireAt { key: Vec<u8>, absolute_ttl: u64 },
    Persist { key: Vec<u8> },
}

impl WriteCommand {
    pub fn set(key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self::Set {
            key: key.into(),
            value: value.into(),
        }
    }

    pub fn delete(key: impl Into<Vec<u8>>) -> Self {
        Self::Delete { key: key.into() }
    }

    pub fn expire(key: impl Into<Vec<u8>>, relative_ttl: u64) -> Self {
        Self::Expire {
            key: key.into(),
            relative_ttl,
        }
    }

    pub fn expire_at(key: impl Into<Vec<u8>>, absolute_ttl: u64) -> Self {
        Self::ExpireAt {
            key: key.into(),
            absolute_ttl,
        }
    }

    pub fn persist(key: impl Into<Vec<u8>>) -> Self {
        Self::Persist { key: key.into() }
    }
}
