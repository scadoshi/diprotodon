//! The domain service — the orchestrator that sits behind the [`CacheService`] port.
//!
//! [`Service`] is the single place that knows how a command becomes both an in-memory
//! effect *and* a durable one. It holds the shared [`Cache`] and a [`CacheRepository`]
//! handle, and composes them: apply to the cache, then (for mutations) append to the log.
//!
//! It's generic over `CR: CacheRepository` so the concrete persister is a plug-in — the
//! service depends only on the port, and tests can substitute a recording fake.

use crate::domain::{
    cache::Cache,
    command::{cache::CacheCommand, outcome::CommandOutcome},
    ports::{CacheRepository, CacheService, ServiceError},
};

/// Command orchestrator. Owns the shared cache and a repository handle, and is itself
/// cheaply [`Clone`]-able (both fields are shared handles) so every session thread can
/// hold its own copy pointing at the same cache and log.
#[derive(Clone)]
pub struct Service<CR: CacheRepository> {
    cache: Cache,
    cache_repo: CR,
}

impl<CR: CacheRepository> Service<CR> {
    /// Wire a service from a shared cache and a repository. Both are shared handles, so
    /// this is a cheap composition, not a deep copy.
    pub fn new(cache: Cache, cache_repo: CR) -> Self {
        Self { cache, cache_repo }
    }
}

impl<CR: CacheRepository> CacheService for Service<CR> {
    /// Run a command against the cache only. No persistence — this is the primitive the
    /// replay path uses so rebuilding from the log doesn't re-write the log.
    fn execute(&self, command: &CacheCommand) -> Result<CommandOutcome, ServiceError> {
        let outcome = self.cache.execute(command)?;
        Ok(outcome)
    }

    /// Run a command and, if it is a [`CacheCommand::Write`], append it to the log.
    ///
    /// Reads and writes are already distinguished by the [`CacheCommand`] variant, so the
    /// command is executed first and then logged only on the `Write` arm. Ordering matters:
    /// the cache effect and the log append must land in the same order across concurrent
    /// writers, or replay would diverge from the live cache. Note the cache lock and the log
    /// lock are taken independently here, so a single lock spanning apply+append (or a
    /// single-writer model) is what would fully close that window under heavy concurrency.
    ///
    /// [`CacheCommand::Write`]: crate::domain::command::cache::CacheCommand::Write
    fn execute_logged(&self, command: CacheCommand) -> Result<CommandOutcome, ServiceError> {
        let outcome = self.execute(&command)?;
        if let CacheCommand::Write(wc) = command {
            self.cache_repo.append(wc)?;
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            cache::Entry,
            command::cache::{read::ReadCommand, write::WriteCommand},
        },
        test_support::RecordingRepo,
    };

    fn build() -> (Service<RecordingRepo>, Cache, RecordingRepo) {
        let cache = Cache::default();
        let repo = RecordingRepo::default();
        let service = Service::new(cache.clone(), repo.clone());
        (service, cache, repo)
    }

    // ---------- execute ----------

    #[test]
    fn execute_get_miss_returns_value_none() {
        let (svc, _, _) = build();
        assert!(matches!(
            svc.execute(&ReadCommand::get("missing").into()).unwrap(),
            CommandOutcome::Value(None)
        ));
    }

    // #[test]
    // fn execute_set_writes_to_cache() {
    //     let (svc, cache, _) = build();
    //     svc.execute(&WriteCommand::set("foo", "bar").into()).unwrap();
    //     assert_eq!(cache.get("foo").unwrap(), Some(Entry::new("bar", None)));
    // }

    // #[test]
    // fn execute_does_not_append_to_repo() {
    //     let (svc, _, repo) = build();
    //     svc.execute(&WriteCommand::set("foo", "bar").into()).unwrap();
    //     svc.execute(&WriteCommand::delete("foo").into()).unwrap();
    //     svc.execute(&WriteCommand::expire("foo", 60).into()).unwrap();
    //     assert!(repo.appended.lock().unwrap().is_empty());
    // }

    // ---------- execute_logged ----------

    // #[test]
    // fn execute_logged_set_appends() {
    //     let (svc, _, repo) = build();
    //     svc.execute_logged(WriteCommand::set("foo", "bar").into())
    //         .unwrap();
    //     let log = repo.appended.lock().unwrap();
    //     assert_eq!(log.len(), 1);
    //     assert!(matches!(
    //         &log[0],
    //         WriteCommand::Set { key, value } if key == b"foo" && value == b"bar"
    //     ));
    // }

    #[test]
    fn execute_logged_delete_appends() {
        let (svc, _, repo) = build();
        svc.execute_logged(WriteCommand::delete("foo").into())
            .unwrap();
        let log = repo.appended.lock().unwrap();
        assert!(matches!(&log[0], WriteCommand::Delete { key } if key == b"foo"));
    }

    #[test]
    fn execute_logged_expire_at_appends() {
        let (svc, cache, repo) = build();
        cache.insert("foo", Entry::new("bar", None)).unwrap();
        svc.execute_logged(WriteCommand::expire_at("foo", u64::MAX).into())
            .unwrap();
        let log = repo.appended.lock().unwrap();
        assert!(matches!(
            &log[0],
            WriteCommand::ExpireAt { key, absolute_ttl }
                if key == b"foo" && *absolute_ttl == u64::MAX
        ));
    }

    #[test]
    fn execute_logged_persist_appends() {
        let (svc, _, repo) = build();
        svc.execute_logged(WriteCommand::persist("foo").into())
            .unwrap();
        let log = repo.appended.lock().unwrap();
        assert!(matches!(&log[0], WriteCommand::Persist { key } if key == b"foo"));
    }

    #[test]
    fn execute_logged_get_does_not_append() {
        let (svc, _, repo) = build();
        svc.execute_logged(ReadCommand::get("foo").into()).unwrap();
        assert!(repo.appended.lock().unwrap().is_empty());
    }

    #[test]
    fn execute_logged_exists_does_not_append() {
        let (svc, _, repo) = build();
        svc.execute_logged(ReadCommand::exists("foo").into())
            .unwrap();
        assert!(repo.appended.lock().unwrap().is_empty());
    }

    #[test]
    fn execute_logged_ttl_does_not_append() {
        let (svc, _, repo) = build();
        svc.execute_logged(ReadCommand::ttl("foo").into()).unwrap();
        assert!(repo.appended.lock().unwrap().is_empty());
    }

    // #[test]
    // fn execute_logged_set_returns_ok_outcome() {
    //     let (svc, _, _) = build();
    //     assert!(matches!(
    //         svc.execute_logged(WriteCommand::set("foo", "bar").into())
    //             .unwrap(),
    //         CommandOutcome::Ok
    //     ));
    // }
}
