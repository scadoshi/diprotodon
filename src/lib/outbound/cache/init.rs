use crate::domain::cache::{Cache, CacheError};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub trait Init {
    fn init(path: impl Into<PathBuf>) -> Result<Self, CacheError>
    where
        Self: Sized;
}

impl Init for Cache {
    fn init(path: impl Into<PathBuf>) -> Result<Self, CacheError> {
        let buf = match File::open(path.into()) {
            Ok(mut file) => {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;
                buf
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let entries = if buf.is_empty() {
            HashMap::new()
        } else {
            wincode::deserialize(&buf)?
        };
        Ok(Self::new(Arc::new(Mutex::new(entries))))
    }
}
