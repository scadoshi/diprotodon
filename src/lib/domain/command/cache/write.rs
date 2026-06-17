#[derive(Debug, Clone, PartialEq)]
pub enum SetExpiry {
    ExAt(u64),
    PxAt(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum WriteCommand {
    Set {
        key: Vec<u8>,
        value: Vec<u8>,
        options: Option<SetExpiry>,
    },
    Delete {
        key: Vec<u8>,
    },
    ExpireAt {
        key: Vec<u8>,
        absolute_ttl: u64,
    },
    Persist {
        key: Vec<u8>,
    },
}

impl WriteCommand {
    pub fn set(
        key: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
        options: Option<SetExpiry>,
    ) -> Self {
        Self::Set {
            key: key.into(),
            value: value.into(),
            options,
        }
    }

    pub fn delete(key: impl Into<Vec<u8>>) -> Self {
        Self::Delete { key: key.into() }
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
