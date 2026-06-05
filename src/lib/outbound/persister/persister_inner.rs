//! Shared file-handle plumbing for the persistence pieces.
//!
//! Both [`Aof`](super::aof::Aof) and [`Snapshot`](super::snapshot::Snapshot) wrap a
//! `PersisterInner` (via newtype + `Deref`), reusing the same "open a file, guard a
//! buffered writer, remember the path" machinery without sharing behavior. The writer is
//! `Arc<Mutex<…>>` so a single underlying file handle is shared across threads and writes
//! are serialized; the `path` is kept for the read/recovery paths that open their own
//! transient handles.

use std::{
    fs::{File, OpenOptions, create_dir_all},
    io::BufWriter,
    path::PathBuf,
    sync::{Arc, Mutex},
};

type IoError = std::io::Error;

/// A guarded, append-mode writer plus the path it points at. The fields are
/// `pub(super)` so the `Aof`/`Snapshot` newtypes in sibling modules can reach the writer
/// and path directly.
#[derive(Debug, Clone)]
pub struct PersisterInner {
    pub(super) writer: Arc<Mutex<BufWriter<File>>>,
    pub(super) path: PathBuf,
}

impl TryFrom<PathBuf> for PersisterInner {
    type Error = IoError;
    /// Open the file in **append** mode (creating it, and any missing parent dirs, if
    /// absent). Append mode is what makes the AOF correct on restart — the write cursor
    /// starts at end-of-file, so reopening an existing log extends it rather than
    /// overwriting from offset 0.
    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        if let Some(dir) = value.parent()
            && !dir.as_os_str().is_empty()
        {
            create_dir_all(dir)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .append(true)
            .open(value.clone())?;
        let writer = Arc::new(Mutex::new(BufWriter::new(file)));
        Ok(Self {
            writer,
            path: value,
        })
    }
}
