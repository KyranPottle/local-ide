use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use protocol::SessionId;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveRun {
    session: SessionId,
    child: Option<SessionId>,
}

// The single run of the engine, because the GPU holds one model at a time
#[derive(Debug, Default)]
pub struct RunLock {
    active: Mutex<Option<ActiveRun>>,
}

// Holds the run lock until it drops, on each exit path of the run
#[derive(Debug)]
pub struct RunGuard {
    lock: Arc<RunLock>,
}

impl RunLock {
    pub fn acquire(self: &Arc<Self>, session: &str) -> Result<RunGuard, SessionId> {
        let mut active = self.state();
        if let Some(run) = active.as_ref() {
            return Err(run.session.clone());
        }
        *active = Some(ActiveRun {
            session: session.to_owned(),
            child: None,
        });
        Ok(RunGuard { lock: self.clone() })
    }

    pub fn active_session(&self) -> Option<SessionId> {
        self.state().as_ref().map(|run| run.session.clone())
    }

    pub fn unless_active<T>(&self, session: &str, change: impl FnOnce() -> T) -> Option<T> {
        let active = self.state();
        let busy = active
            .as_ref()
            .is_some_and(|run| run.session == session || run.child.as_deref() == Some(session));
        if busy {
            return None;
        }
        let result = change();
        drop(active);
        Some(result)
    }

    fn state(&self) -> MutexGuard<'_, Option<ActiveRun>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl RunGuard {
    pub fn set_child(&self, child: Option<SessionId>) {
        if let Some(run) = self.lock.state().as_mut() {
            run.child = child;
        }
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        *self.lock.state() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_acquire_reports_the_active_session() {
        let lock = Arc::new(RunLock::default());
        let _guard = lock.acquire("a").unwrap();
        assert_eq!(lock.acquire("b").unwrap_err(), "a");
        assert_eq!(lock.active_session().as_deref(), Some("a"));
    }

    #[test]
    fn dropping_the_guard_releases_the_lock() {
        let lock = Arc::new(RunLock::default());
        drop(lock.acquire("a").unwrap());
        assert_eq!(lock.active_session(), None);
        assert!(lock.acquire("b").is_ok());
    }

    #[test]
    fn a_panic_that_unwinds_the_guard_releases_the_lock() {
        let lock = Arc::new(RunLock::default());
        let panicking = lock.clone();
        let outcome = std::thread::spawn(move || {
            let _guard = panicking.acquire("a").unwrap();
            panic!("run failed");
        })
        .join();
        assert!(outcome.is_err());
        assert_eq!(lock.active_session(), None);
    }

    #[test]
    fn the_session_and_its_running_child_are_busy() {
        let lock = Arc::new(RunLock::default());
        let guard = lock.acquire("parent").unwrap();
        guard.set_child(Some("child".to_owned()));
        assert_eq!(lock.unless_active("parent", || 1), None);
        assert_eq!(lock.unless_active("child", || 1), None);
        assert_eq!(lock.unless_active("other", || 1), Some(1));
        guard.set_child(None);
        assert_eq!(lock.unless_active("child", || 1), Some(1));
        drop(guard);
        assert_eq!(lock.unless_active("parent", || 1), Some(1));
    }
}
