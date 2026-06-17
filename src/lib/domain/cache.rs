//! In-memory key-value store with TTL support and lazy + active expiry.
//!
//! [`Cache`] is the only type a caller needs. It wraps `Arc<Mutex<HashMap<Vec<u8>, Entry>>>`
//! so handles are cheap to clone across threads and the underlying map is shared. Every
//! read path drops expired keys it encounters (lazy expiry); a separate sweeper thread
//! in `inbound::server` calls [`Cache::remove_expired`] periodically to reclaim keys no
//! one has touched.
//!
//! ## Expiry model
//!
//! TTLs are stored as absolute UNIX seconds inside each [`Entry`]. Relative TTLs are
//! converted to absolute at the API boundary, so the storage representation is uniform
//! and serializable. `SystemTime` (not `Instant`) is used because `Instant` is
//! process-local and meaningless across a restart — wall-clock skew at second-level
//! granularity is acceptable for a TTL.
//!
//! ## Persistence
//!
//! The cache itself is purely in-memory and has no persistence methods. Durability lives
//! in the outbound persister (`outbound::persister`): a `wincode`-serialized snapshot of
//! the `HashMap<Vec<u8>, Entry>` plus an append-only command log replayed on startup.

use crate::domain::command::{
    cache::{CacheCommand, read::ReadCommand, write::WriteCommand},
    outcome::{CommandOutcome, TtlOutcome},
};
use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, Mutex},
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};
use thiserror::Error;
use wincode::{SchemaRead, SchemaWrite};

type CO = CommandOutcome;

/// Errors a [`Cache`] operation can return.
#[derive(Debug, Error)]
pub enum CacheError {
    /// The shared mutex was poisoned by a thread that panicked while holding it.
    /// Poisoning means the in-memory state may be inconsistent; the cache cannot be
    /// safely used after this.
    #[error("cache mutex poisoned")]
    MutexPoisoned,
    /// `SystemTime::now()` returned a value before the UNIX epoch — only possible if
    /// the system clock is badly wrong.
    #[error(transparent)]
    SystemTime(#[from] SystemTimeError),
}

/// A single cache entry: opaque value bytes plus an optional absolute TTL.
///
/// `absolute_ttl` is UNIX seconds (wall clock). `None` means the entry has no expiry
/// and lives until explicitly removed. Past timestamps are accepted but a subsequent
/// read on this key will treat the entry as expired and drop it (lazy expiry).
#[derive(Clone, Debug, Default, SchemaWrite, SchemaRead, PartialEq, Hash)]
pub struct Entry {
    /// Opaque payload bytes — UTF-8 is never enforced so values can be jpegs, RESP
    /// fragments, anything.
    pub value: Vec<u8>,
    /// Absolute UNIX seconds at which this entry expires, or `None` for no TTL.
    pub absolute_ttl: Option<u64>,
}

impl Entry {
    /// Build an [`Entry`] from convertible value bytes plus an optional absolute TTL.
    /// To set a relative TTL, build with `None` and call [`Cache::set_relative_ttl`]
    /// afterwards — the cache layer handles the now+seconds arithmetic.
    pub fn new(value: impl Into<Vec<u8>>, absolute_ttl: Option<u64>) -> Self {
        Self {
            value: value.into(),
            absolute_ttl,
        }
    }
}

/// Thread-safe shared in-memory KV store.
///
/// Cloning a `Cache` clones the underlying `Arc` — every clone refers to the same
/// backing map. This is how the server hands the cache to its connection threads,
/// sweeper, and persistence loop.
///
/// `Default` yields an empty in-memory cache backed by a fresh `Arc<Mutex<…>>`; use it
/// for tests. To rebuild from disk, construct via [`Cache::new`] with a snapshot-loaded
/// map and replay the log through the outbound persister.
#[derive(Clone, Debug, Default)]
pub struct Cache {
    inner: Arc<Mutex<HashMap<Vec<u8>, Entry>>>,
}

impl Deref for Cache {
    type Target = Arc<Mutex<HashMap<Vec<u8>, Entry>>>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Cache {
    pub fn new(data: HashMap<Vec<u8>, Entry>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(data)),
        }
    }

    /// Fetch a clone of the [`Entry`] for `key`, or `None` if missing or expired.
    ///
    /// If the entry exists but its `absolute_ttl` has passed, this returns `None` *and*
    /// removes the entry — that's the lazy-expiry contract. Lock is held across the
    /// expiry check and removal so a concurrent insert can't race in between.
    pub fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Entry>, CacheError> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        let entry = guard.get(key.as_ref()).cloned();
        if entry
            .as_ref()
            .is_some_and(|e| e.absolute_ttl.is_some_and(|t| now >= t))
        {
            guard.remove(key.as_ref());
            Ok(None)
        } else {
            Ok(entry)
        }
    }

    /// Insert or replace `key` with `entry`. Returns the previous entry if one existed,
    /// `None` if `key` is new.
    ///
    /// An entry whose `absolute_ttl` is already in the past is accepted as-is — the next
    /// read drops it lazily. The caller does not need to clock-check before inserting.
    pub fn insert(
        &self,
        key: impl Into<Vec<u8>>,
        entry: Entry,
    ) -> Result<Option<Entry>, CacheError> {
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        Ok(guard.insert(key.into(), entry))
    }

    /// Remove `key` from the cache. Returns the removed entry if one existed.
    /// Does not consult TTL — this is an unconditional delete.
    pub fn remove(&self, key: impl AsRef<[u8]>) -> Result<Option<Entry>, CacheError> {
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        Ok(guard.remove(key.as_ref()))
    }

    /// Test for `key`'s presence. Honors lazy expiry: an expired key returns `false`
    /// and is dropped from the map on its way out, matching what a subsequent
    /// [`Cache::get`] would see.
    pub fn contains(&self, key: impl AsRef<[u8]>) -> Result<bool, CacheError> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        let entry = guard.get(key.as_ref());
        match entry {
            Some(Entry {
                absolute_ttl: Some(t),
                ..
            }) if now >= *t => {
                guard.remove(key.as_ref());
                Ok(false)
            }
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Set an absolute TTL (UNIX seconds) on an existing key. Returns `true` if the
    /// TTL was applied — including the immediate-deletion case where the timestamp is
    /// already past and `key` existed. Returns `false` if `key` was missing.
    ///
    /// A past timestamp triggers immediate removal rather than storing an
    /// already-expired entry. This matches real Redis EXPIREAT semantics and means
    /// the lazy-expiry path doesn't have to handle a guaranteed-stale case.
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
        let Some(entry) = guard.get_mut(key.as_ref()) else {
            return Ok(false);
        };
        entry.absolute_ttl = Some(absolute_ttl);
        Ok(true)
    }

    /// Set a relative TTL (seconds-from-now) on an existing key. Converts to absolute
    /// and delegates to [`Cache::set_absolute_ttl`]. `saturating_add` on the conversion
    /// guards against `u64` overflow on huge TTLs.
    ///
    /// `relative_ttl == 0` produces an absolute timestamp equal to *now*, which the
    /// `>=` check inside [`Cache::set_absolute_ttl`] treats as past — so `EXPIRE foo 0`
    /// deletes immediately, matching real Redis.
    pub fn set_relative_ttl(
        &self,
        key: impl AsRef<[u8]>,
        relative_ttl: u64,
    ) -> Result<bool, CacheError> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let absolute_ttl = now.saturating_add(relative_ttl);
        self.set_absolute_ttl(key, absolute_ttl)
    }

    /// Query the absolute TTL for `key`. The double `Option` carries three distinct
    /// states:
    ///
    /// - `Ok(None)` — `key` is missing (or was expired and just got swept here).
    /// - `Ok(Some(None))` — `key` exists but has no TTL set.
    /// - `Ok(Some(Some(t)))` — `key` exists with absolute UNIX timestamp `t`.
    ///
    /// Honors lazy expiry: if the stored TTL has passed, the entry is removed and the
    /// function returns `Ok(None)`.
    pub fn get_absolute_ttl(
        &self,
        key: impl AsRef<[u8]>,
    ) -> Result<Option<Option<u64>>, CacheError> {
        let guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        let Some(Entry { absolute_ttl, .. }) = guard.get(key.as_ref()) else {
            return Ok(None);
        };
        let ttl = absolute_ttl.to_owned();
        drop(guard);
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        if ttl.is_some_and(|t| now >= t) {
            self.remove(key)?;
            return Ok(None);
        }
        Ok(Some(ttl))
    }

    /// Query the relative TTL (seconds remaining) for `key`. Same three-state shape as
    /// [`Cache::get_absolute_ttl`]:
    ///
    /// - `Ok(None)` — `key` missing or just-expired.
    /// - `Ok(Some(None))` — `key` exists with no TTL.
    /// - `Ok(Some(Some(n)))` — `n` seconds remaining (saturating; can't underflow).
    ///
    /// Drives the wire `TTL` command reply: `:-2` / `:-1` / `:n`.
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

    /// Drop the TTL on `key` (keep the value). Returns `true` if a TTL was actually
    /// removed, `false` if `key` is missing or already had no TTL.
    ///
    /// Drives `PERSIST`: `:1` when a TTL was cleared, `:0` otherwise.
    pub fn remove_ttl(&self, key: impl AsRef<[u8]>) -> Result<bool, CacheError> {
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        let Some(entry) = guard.get_mut(key.as_ref()) else {
            return Ok(false);
        };
        if entry.absolute_ttl.is_some() {
            entry.absolute_ttl = None;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Sweep all expired entries. Returns the count of keys removed. Called by the
    /// background sweeper thread in `inbound::server` on a periodic tick.
    ///
    /// The lock is held for the full scan-and-delete pass. At current scale the sweep
    /// is microseconds; if it ever starts to hurt tail latency, switch to a
    /// snapshot-then-evict pattern with a TTL re-check during the eviction phase.
    pub fn remove_expired(&self) -> Result<usize, CacheError> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let mut guard = self.inner.lock().map_err(|_| CacheError::MutexPoisoned)?;
        let expired: Vec<Vec<u8>> = guard
            .iter()
            .filter(|(_, Entry { absolute_ttl, .. })| absolute_ttl.is_some_and(|t| now >= t))
            .map(|(key, _)| key.to_owned())
            .collect();
        for key in expired.iter() {
            guard.remove(key);
        }
        Ok(expired.len())
    }

    pub fn execute(&self, command: &CacheCommand) -> Result<CommandOutcome, CacheError> {
        use ReadCommand as R;
        use WriteCommand as W;
        let outcome = match command {
            CacheCommand::Read(R::Get { key }) => match self.get(key)? {
                Some(Entry { value, .. }) => CO::Value(Some(value)),
                None => CO::Value(None),
            },
            CacheCommand::Read(R::Exists { key }) => CO::Integer(self.contains(key)? as i64),
            CacheCommand::Read(R::Ttl { key }) => CO::Ttl(match self.get_relative_ttl(key)? {
                None => TtlOutcome::KeyNotFound,
                Some(None) => TtlOutcome::TtlNotFound,
                Some(Some(ttl)) => TtlOutcome::Some(ttl),
            }),
            CacheCommand::Write(W::Set { key, value }) => {
                self.insert(key.as_slice(), Entry::new(value.as_slice(), None))?;
                CO::Ok
            }
            CacheCommand::Write(W::Delete { key }) => CO::Bool(self.remove(key)?.is_some()),
            CacheCommand::Write(W::Expire { key, relative_ttl }) => {
                CO::Bool(self.set_relative_ttl(key, *relative_ttl)?)
            }
            CacheCommand::Write(W::ExpireAt { key, absolute_ttl }) => {
                CO::Bool(self.set_absolute_ttl(key, *absolute_ttl)?)
            }
            CacheCommand::Write(W::Persist { key }) => CO::Bool(self.remove_ttl(key)?),
        };
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- helpers ----------

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    // ---------- Entry ----------

    #[test]
    fn entry_constructor_without_ttl() {
        assert_eq!(
            Entry::new("foo", None),
            Entry {
                value: b"foo".to_vec(),
                absolute_ttl: None
            }
        );
    }

    #[test]
    fn entry_constructor_with_ttl() {
        assert_eq!(
            Entry::new("foo", Some(123)),
            Entry {
                value: b"foo".to_vec(),
                absolute_ttl: Some(123)
            }
        );
    }

    // ---------- get ----------

    #[test]
    fn get_miss_returns_none() {
        let cache = Cache::default();
        assert_eq!(cache.get("missing").unwrap(), None);
    }

    #[test]
    fn get_hit_no_ttl_returns_entry() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert_eq!(cache.get("foo").unwrap(), Some(Entry::new("bar", None)));
    }

    #[test]
    fn get_hit_future_ttl_returns_entry() {
        let cache = Cache::default();
        let future = now() + 3600;
        cache
            .insert("foo", Entry::new("bar", Some(future)))
            .unwrap();
        assert_eq!(
            cache.get("foo").unwrap(),
            Some(Entry::new("bar", Some(future)))
        );
    }

    #[test]
    fn get_hit_expired_returns_none_and_removes() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", Some(1))).unwrap();
        assert_eq!(cache.get("foo").unwrap(), None);
        // Verify the lazy expiry actually removed it.
        assert!(!cache.contains("foo").unwrap());
    }

    #[test]
    fn get_hit_at_exact_expiry_returns_none() {
        let cache = Cache::default();
        let t = now();
        cache.insert("foo", Entry::new("bar", Some(t))).unwrap();
        // now >= t triggers the expired branch.
        assert_eq!(cache.get("foo").unwrap(), None);
    }

    // ---------- insert ----------

    #[test]
    fn insert_new_returns_none() {
        let cache = Cache::default();
        assert_eq!(cache.insert("foo", Entry::new("bar", None)).unwrap(), None);
    }

    #[test]
    fn insert_existing_returns_old_entry() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("old", None)).unwrap();
        assert_eq!(
            cache.insert("foo", Entry::new("new", None)).unwrap(),
            Some(Entry::new("old", None))
        );
    }

    #[test]
    fn insert_replaces_value_and_ttl() {
        let cache = Cache::default();
        cache
            .insert("foo", Entry::new("old", Some(now() + 100)))
            .unwrap();
        cache.insert("foo", Entry::new("new", None)).unwrap();
        assert_eq!(cache.get("foo").unwrap(), Some(Entry::new("new", None)));
    }

    // ---------- remove ----------

    #[test]
    fn remove_hit_returns_entry() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert_eq!(cache.remove("foo").unwrap(), Some(Entry::new("bar", None)));
    }

    #[test]
    fn remove_miss_returns_none() {
        let cache = Cache::default();
        assert_eq!(cache.remove("missing").unwrap(), None);
    }

    #[test]
    fn remove_actually_removes() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        cache.remove("foo").unwrap();
        assert_eq!(cache.get("foo").unwrap(), None);
    }

    // ---------- contains ----------

    #[test]
    fn contains_miss_returns_false() {
        let cache = Cache::default();
        assert!(!cache.contains("missing").unwrap());
    }

    #[test]
    fn contains_hit_no_ttl_returns_true() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert!(cache.contains("foo").unwrap());
    }

    #[test]
    fn contains_hit_future_ttl_returns_true() {
        let cache = Cache::default();
        cache
            .insert("foo", Entry::new("bar", Some(now() + 3600)))
            .unwrap();
        assert!(cache.contains("foo").unwrap());
    }

    #[test]
    fn contains_hit_expired_returns_false_and_removes() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", Some(1))).unwrap();
        assert!(!cache.contains("foo").unwrap());
        // Subsequent direct lookups confirm the entry is gone.
        assert_eq!(cache.get("foo").unwrap(), None);
    }

    // ---------- set_absolute_ttl ----------

    #[test]
    fn set_absolute_ttl_existing_key_future_returns_true_and_sets() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        let future = now() + 3600;
        assert!(cache.set_absolute_ttl("foo", future).unwrap());
        assert_eq!(
            cache.get("foo").unwrap(),
            Some(Entry::new("bar", Some(future)))
        );
    }

    #[test]
    fn set_absolute_ttl_missing_key_future_returns_false() {
        let cache = Cache::default();
        assert!(!cache.set_absolute_ttl("missing", now() + 3600).unwrap());
    }

    #[test]
    fn set_absolute_ttl_past_existing_key_removes_and_returns_true() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert!(cache.set_absolute_ttl("foo", 0).unwrap());
        assert_eq!(cache.get("foo").unwrap(), None);
    }

    #[test]
    fn set_absolute_ttl_past_missing_key_returns_false() {
        let cache = Cache::default();
        assert!(!cache.set_absolute_ttl("missing", 0).unwrap());
    }

    #[test]
    fn set_absolute_ttl_overwrites_existing_ttl() {
        let cache = Cache::default();
        cache
            .insert("foo", Entry::new("bar", Some(now() + 100)))
            .unwrap();
        let new_ttl = now() + 7200;
        assert!(cache.set_absolute_ttl("foo", new_ttl).unwrap());
        assert_eq!(cache.get_absolute_ttl("foo").unwrap(), Some(Some(new_ttl)));
    }

    // ---------- set_relative_ttl ----------

    #[test]
    fn set_relative_ttl_existing_key_returns_true_and_sets_absolute() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        let before = now();
        assert!(cache.set_relative_ttl("foo", 3600).unwrap());
        let after = now();
        let ttl = match cache.get_absolute_ttl("foo").unwrap() {
            Some(Some(t)) => t,
            other => panic!("expected Some(Some(_)), got {:?}", other),
        };
        // Allow for the clock to tick during the call.
        assert!(ttl >= before + 3600 && ttl <= after + 3600);
    }

    #[test]
    fn set_relative_ttl_missing_key_returns_false() {
        let cache = Cache::default();
        assert!(!cache.set_relative_ttl("missing", 3600).unwrap());
    }

    #[test]
    fn set_relative_ttl_zero_seconds_removes_immediately() {
        // EXPIRE key 0 → absolute_ttl == now → set_absolute_ttl removes (matches real Redis).
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert!(cache.set_relative_ttl("foo", 0).unwrap());
        assert_eq!(cache.get("foo").unwrap(), None);
    }

    // ---------- get_absolute_ttl ----------

    #[test]
    fn get_absolute_ttl_missing_key_returns_none() {
        let cache = Cache::default();
        assert_eq!(cache.get_absolute_ttl("missing").unwrap(), None);
    }

    #[test]
    fn get_absolute_ttl_no_ttl_returns_some_none() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert_eq!(cache.get_absolute_ttl("foo").unwrap(), Some(None));
    }

    #[test]
    fn get_absolute_ttl_future_returns_some_some_timestamp() {
        let cache = Cache::default();
        let future = now() + 3600;
        cache
            .insert("foo", Entry::new("bar", Some(future)))
            .unwrap();
        assert_eq!(cache.get_absolute_ttl("foo").unwrap(), Some(Some(future)));
    }

    #[test]
    fn get_absolute_ttl_expired_removes_and_returns_none() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", Some(1))).unwrap();
        assert_eq!(cache.get_absolute_ttl("foo").unwrap(), None);
        // Confirm the lazy expiry physically removed the entry.
        assert!(!cache.contains("foo").unwrap());
    }

    // ---------- get_relative_ttl ----------

    #[test]
    fn get_relative_ttl_missing_key_returns_none() {
        let cache = Cache::default();
        assert_eq!(cache.get_relative_ttl("missing").unwrap(), None);
    }

    #[test]
    fn get_relative_ttl_no_ttl_returns_some_none() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert_eq!(cache.get_relative_ttl("foo").unwrap(), Some(None));
    }

    #[test]
    fn get_relative_ttl_future_returns_remaining_seconds() {
        let cache = Cache::default();
        let future = now() + 3600;
        cache
            .insert("foo", Entry::new("bar", Some(future)))
            .unwrap();
        let remaining = match cache.get_relative_ttl("foo").unwrap() {
            Some(Some(t)) => t,
            other => panic!("expected Some(Some(_)), got {:?}", other),
        };
        // Should be ~3600s; allow some slack for clock tick during the call.
        assert!((3590_u64..=3600).contains(&remaining));
    }

    #[test]
    fn get_relative_ttl_expired_returns_none() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", Some(1))).unwrap();
        assert_eq!(cache.get_relative_ttl("foo").unwrap(), None);
    }

    // ---------- remove_ttl ----------

    #[test]
    fn remove_ttl_had_ttl_returns_true_and_clears() {
        let cache = Cache::default();
        cache
            .insert("foo", Entry::new("bar", Some(now() + 100)))
            .unwrap();
        assert!(cache.remove_ttl("foo").unwrap());
        assert_eq!(cache.get_absolute_ttl("foo").unwrap(), Some(None));
    }

    #[test]
    fn remove_ttl_no_ttl_returns_false() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert!(!cache.remove_ttl("foo").unwrap());
    }

    #[test]
    fn remove_ttl_missing_key_returns_false() {
        let cache = Cache::default();
        assert!(!cache.remove_ttl("missing").unwrap());
    }

    #[test]
    fn remove_ttl_keeps_value_intact() {
        let cache = Cache::default();
        cache
            .insert("foo", Entry::new("bar", Some(now() + 100)))
            .unwrap();
        cache.remove_ttl("foo").unwrap();
        assert_eq!(cache.get("foo").unwrap(), Some(Entry::new("bar", None)));
    }

    // ---------- remove_expired ----------

    #[test]
    fn remove_expired_empty_returns_zero() {
        let cache = Cache::default();
        assert_eq!(cache.remove_expired().unwrap(), 0);
    }

    #[test]
    fn remove_expired_drops_expired_keys() {
        let cache = Cache::default();
        cache.insert("a", Entry::new("v", Some(1))).unwrap();
        cache.insert("b", Entry::new("v", Some(1))).unwrap();
        assert_eq!(cache.remove_expired().unwrap(), 2);
        assert!(!cache.contains("a").unwrap());
        assert!(!cache.contains("b").unwrap());
    }

    #[test]
    fn remove_expired_keeps_future_keys() {
        let cache = Cache::default();
        cache
            .insert("a", Entry::new("v", Some(now() + 3600)))
            .unwrap();
        assert_eq!(cache.remove_expired().unwrap(), 0);
        assert!(cache.contains("a").unwrap());
    }

    #[test]
    fn remove_expired_keeps_no_ttl_keys() {
        let cache = Cache::default();
        cache.insert("a", Entry::new("v", None)).unwrap();
        assert_eq!(cache.remove_expired().unwrap(), 0);
        assert!(cache.contains("a").unwrap());
    }

    // ---------- execute ----------

    fn get(key: impl Into<Vec<u8>>) -> CacheCommand {
        ReadCommand::get(key).into()
    }
    fn set(key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> CacheCommand {
        WriteCommand::set(key, value).into()
    }
    fn delete(key: impl Into<Vec<u8>>) -> CacheCommand {
        WriteCommand::delete(key).into()
    }
    fn exists(key: impl Into<Vec<u8>>) -> CacheCommand {
        ReadCommand::exists(key).into()
    }
    fn expire(key: impl Into<Vec<u8>>, relative_ttl: u64) -> CacheCommand {
        WriteCommand::expire(key, relative_ttl).into()
    }
    fn expire_at(key: impl Into<Vec<u8>>, absolute_ttl: u64) -> CacheCommand {
        WriteCommand::expire_at(key, absolute_ttl).into()
    }
    fn ttl(key: impl Into<Vec<u8>>) -> CacheCommand {
        ReadCommand::ttl(key).into()
    }
    fn persist(key: impl Into<Vec<u8>>) -> CacheCommand {
        WriteCommand::persist(key).into()
    }

    #[test]
    fn execute_get_miss_returns_value_none() {
        let cache = Cache::default();
        assert!(matches!(
            cache.execute(&get("missing")).unwrap(),
            CommandOutcome::Value(None)
        ));
    }

    #[test]
    fn execute_get_hit_returns_value_some() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        match cache.execute(&get("foo")).unwrap() {
            CommandOutcome::Value(Some(v)) => assert_eq!(v, b"bar".to_vec()),
            other => panic!("expected Value(Some), got {:?}", other),
        }
    }

    #[test]
    fn execute_get_expired_returns_value_none() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", Some(1))).unwrap();
        assert!(matches!(
            cache.execute(&get("foo")).unwrap(),
            CommandOutcome::Value(None)
        ));
    }

    #[test]
    fn execute_set_returns_ok_and_inserts() {
        let cache = Cache::default();
        assert!(matches!(
            cache.execute(&set("foo", "bar")).unwrap(),
            CommandOutcome::Ok
        ));
        assert_eq!(cache.get("foo").unwrap(), Some(Entry::new("bar", None)));
    }

    #[test]
    fn execute_set_overwrites() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("old", None)).unwrap();
        cache.execute(&set("foo", "new")).unwrap();
        assert_eq!(cache.get("foo").unwrap(), Some(Entry::new("new", None)));
    }

    #[test]
    fn execute_delete_hit_returns_bool_true() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert!(matches!(
            cache.execute(&delete("foo")).unwrap(),
            CommandOutcome::Bool(true)
        ));
        assert!(!cache.contains("foo").unwrap());
    }

    #[test]
    fn execute_delete_miss_returns_bool_false() {
        let cache = Cache::default();
        assert!(matches!(
            cache.execute(&delete("missing")).unwrap(),
            CommandOutcome::Bool(false)
        ));
    }

    #[test]
    fn execute_exists_hit_returns_integer_one() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert!(matches!(
            cache.execute(&exists("foo")).unwrap(),
            CommandOutcome::Integer(1)
        ));
    }

    #[test]
    fn execute_exists_miss_returns_integer_zero() {
        let cache = Cache::default();
        assert!(matches!(
            cache.execute(&exists("missing")).unwrap(),
            CommandOutcome::Integer(0)
        ));
    }

    #[test]
    fn execute_expire_existing_returns_bool_true() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert!(matches!(
            cache.execute(&expire("foo", 3600)).unwrap(),
            CommandOutcome::Bool(true)
        ));
    }

    #[test]
    fn execute_expire_missing_returns_bool_false() {
        let cache = Cache::default();
        assert!(matches!(
            cache.execute(&expire("missing", 3600)).unwrap(),
            CommandOutcome::Bool(false)
        ));
    }

    #[test]
    fn execute_expire_at_existing_returns_bool_true() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert!(matches!(
            cache.execute(&expire_at("foo", now() + 3600)).unwrap(),
            CommandOutcome::Bool(true)
        ));
    }

    #[test]
    fn execute_expire_at_missing_returns_bool_false() {
        let cache = Cache::default();
        assert!(matches!(
            cache.execute(&expire_at("missing", now() + 3600)).unwrap(),
            CommandOutcome::Bool(false)
        ));
    }

    #[test]
    fn execute_ttl_missing_returns_key_not_found() {
        let cache = Cache::default();
        assert!(matches!(
            cache.execute(&ttl("missing")).unwrap(),
            CommandOutcome::Ttl(TtlOutcome::KeyNotFound)
        ));
    }

    #[test]
    fn execute_ttl_no_ttl_returns_ttl_not_found() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert!(matches!(
            cache.execute(&ttl("foo")).unwrap(),
            CommandOutcome::Ttl(TtlOutcome::TtlNotFound)
        ));
    }

    #[test]
    fn execute_ttl_with_ttl_returns_ttl_some() {
        let cache = Cache::default();
        cache
            .insert("foo", Entry::new("bar", Some(now() + 3600)))
            .unwrap();
        match cache.execute(&ttl("foo")).unwrap() {
            CommandOutcome::Ttl(TtlOutcome::Some(t)) => {
                assert!((3590_u64..=3600).contains(&t));
            }
            other => panic!("expected Ttl(Some), got {:?}", other),
        }
    }

    #[test]
    fn execute_persist_had_ttl_returns_bool_true() {
        let cache = Cache::default();
        cache
            .insert("foo", Entry::new("bar", Some(now() + 3600)))
            .unwrap();
        assert!(matches!(
            cache.execute(&persist("foo")).unwrap(),
            CommandOutcome::Bool(true)
        ));
    }

    #[test]
    fn execute_persist_no_ttl_returns_bool_false() {
        let cache = Cache::default();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        assert!(matches!(
            cache.execute(&persist("foo")).unwrap(),
            CommandOutcome::Bool(false)
        ));
    }

    #[test]
    fn execute_persist_missing_returns_bool_false() {
        let cache = Cache::default();
        assert!(matches!(
            cache.execute(&persist("missing")).unwrap(),
            CommandOutcome::Bool(false)
        ));
    }

    #[test]
    fn remove_expired_mixed() {
        let cache = Cache::default();
        cache.insert("expired_a", Entry::new("v", Some(1))).unwrap();
        cache.insert("expired_b", Entry::new("v", Some(1))).unwrap();
        cache
            .insert("future", Entry::new("v", Some(now() + 3600)))
            .unwrap();
        cache.insert("no_ttl", Entry::new("v", None)).unwrap();
        assert_eq!(cache.remove_expired().unwrap(), 2);
        assert!(!cache.contains("expired_a").unwrap());
        assert!(!cache.contains("expired_b").unwrap());
        assert!(cache.contains("future").unwrap());
        assert!(cache.contains("no_ttl").unwrap());
    }
}
