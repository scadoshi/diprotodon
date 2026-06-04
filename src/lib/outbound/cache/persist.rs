use crate::domain::cache::{Cache, CacheError};
use std::{fs::OpenOptions, io::Write, path::PathBuf};

pub trait Persist {
    fn persist(&self, path: impl Into<PathBuf>) -> Result<(), CacheError>;
}

impl Persist for Cache {
    fn persist(&self, path: impl Into<PathBuf>) -> Result<(), CacheError> {
        let inner = self
            .lock()
            .map_err(|_| CacheError::MutexPoisoned)?
            .to_owned();
        let bytes = wincode::serialize(&inner)?;
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path.into())?;
        file.write_all(&bytes)?;
        Ok(())
    }
}
