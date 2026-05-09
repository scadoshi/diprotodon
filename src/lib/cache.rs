use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{Read, Write},
    sync::{Arc, Mutex},
};
use thiserror::Error;
use wincode::{ReadError, WriteError};

const CACHE_PATH: &str = "cache";

#[derive(Debug, Error)]
pub enum CacheError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    ReadError(#[from] ReadError),
    #[error(transparent)]
    WriteError(#[from] WriteError),
    #[error("cache mutex poisoned")]
    MutexPoisoned,
}

#[derive(Clone, Debug)]
pub struct Cache {
    inner: Arc<Mutex<HashMap<String, String>>>,
}

impl Cache {
    pub fn get(&self, key: impl AsRef<str>) -> Result<Option<String>, CacheError> {
        let guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        Ok(guard.get(key.as_ref()).cloned())
    }

    pub fn set(
        &self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Option<String>, CacheError> {
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        Ok(guard.insert(key.into(), value.into()))
    }

    pub fn delete(&self, key: impl AsRef<str>) -> Result<Option<String>, CacheError> {
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        Ok(guard.remove(key.as_ref()))
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
        let map = if buf.is_empty() {
            HashMap::new()
        } else {
            wincode::deserialize(&buf)?
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(map)),
        })
    }

    pub fn persist(&self) -> Result<(), CacheError> {
        let guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        let bytes = wincode::serialize(&*guard)?;
        drop(guard);
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(CACHE_PATH)?;
        file.write_all(&bytes)?;
        Ok(())
    }
}
