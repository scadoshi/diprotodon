//! RESP serialization. Just [`reply`] — the [`reply::Reply`] enum and the `SimpleInner`
//! newtype that enforces RESP's no-CR/LF rule for simple-string and simple-error frames
//! at construction time.

pub mod reply;
