use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    sync::{Arc, Mutex},
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};
use thiserror::Error;
use wincode::{ReadError, SchemaRead, SchemaWrite, WriteError};

const CACHE_PATH: &str = "cache";

#[derive(Debug, Error)]
pub enum CacheError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Read(#[from] ReadError),
    #[error(transparent)]
    Write(#[from] WriteError),
    #[error("cache mutex poisoned")]
    MutexPoisoned,
    #[error(transparent)]
    SystemTime(#[from] SystemTimeError),
}

#[derive(Clone, Debug, Default, SchemaWrite, SchemaRead)]
struct Inner {
    values: HashMap<Vec<u8>, Vec<u8>>,
    expiries: HashMap<Vec<u8>, u64>,
}

#[derive(Clone, Debug, Default)]
pub struct Cache {
    inner: Arc<Mutex<Inner>>,
}

impl Cache {
    pub fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Vec<u8>>, CacheError> {
        let guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        Ok(guard.values.get(key.as_ref()).cloned())
    }

    pub fn insert(
        &self,
        key: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
    ) -> Result<Option<Vec<u8>>, CacheError> {
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        Ok(guard.values.insert(key.into(), value.into()))
    }

    pub fn remove(&self, key: impl AsRef<[u8]>) -> Result<Option<Vec<u8>>, CacheError> {
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        guard.expiries.remove(key.as_ref());
        Ok(guard.values.remove(key.as_ref()))
    }

    pub fn contains(&self, key: impl AsRef<[u8]>) -> Result<bool, CacheError> {
        let guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        Ok(guard.values.contains_key(key.as_ref()))
    }

    pub fn set_absolute_ttl(
        &self,
        key: impl AsRef<[u8]>,
        absolute_ttl: u64,
    ) -> Result<bool, CacheError> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        if now >= absolute_ttl {
            return Ok(self.remove(key)?.is_some());
        }
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        if guard.values.contains_key(key.as_ref()) {
            guard.expiries.insert(key.as_ref().to_owned(), absolute_ttl);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn set_relative_ttl(
        &self,
        key: impl AsRef<[u8]>,
        relative_ttl: u64,
    ) -> Result<bool, CacheError> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let absolute_ttl = now.saturating_add(relative_ttl);
        self.set_absolute_ttl(key, absolute_ttl)
    }

    pub fn get_absolute_ttl(
        &self,
        key: impl AsRef<[u8]>,
    ) -> Result<Option<Option<u64>>, CacheError> {
        let guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        if !guard.values.contains_key(key.as_ref()) {
            return Ok(None);
        }
        let Some(absolute_ttl) = guard.expiries.get(key.as_ref()).copied() else {
            return Ok(Some(None));
        };
        drop(guard);
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        if now >= absolute_ttl {
            self.remove(key)?;
            return Ok(None);
        }
        Ok(Some(Some(absolute_ttl)))
    }

    pub fn get_relative_ttl(
        &self,
        key: impl AsRef<[u8]>,
    ) -> Result<Option<Option<u64>>, CacheError> {
        let absolute_ttl = match self.get_absolute_ttl(key.as_ref())? {
            None => return Ok(None),
            Some(None) => return Ok(Some(None)),
            Some(Some(t)) => t,
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        Ok(Some(Some(absolute_ttl.saturating_sub(now))))
    }

    pub fn remove_ttl(&self, key: impl AsRef<[u8]>) -> Result<bool, CacheError> {
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        Ok(guard.expiries.remove(key.as_ref()).is_some())
    }

    pub fn init() -> Result<Self, CacheError> {
        let buf = match File::open(CACHE_PATH) {
            Ok(mut file) => {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                buf
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let inner = if buf.is_empty() {
            Inner::default()
        } else {
            wincode::deserialize(&buf)?
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    pub fn persist(&self) -> Result<(), CacheError> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| CacheError::MutexPoisoned)?
            .to_owned();
        let bytes = wincode::serialize(&inner)?;
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(CACHE_PATH)?;
        file.write_all(&bytes)?;
        Ok(())
    }

    pub fn remove_expired(&self) -> Result<usize, CacheError> {
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let expired: Vec<Vec<u8>> = guard
            .expiries
            .iter()
            .filter(|(_, ttl)| now >= **ttl)
            .map(|(key, _)| key.to_owned())
            .collect();
        for key in expired.iter() {
            guard.values.remove(key);
            guard.expiries.remove(key);
        }
        Ok(expired.len())
    }
}
