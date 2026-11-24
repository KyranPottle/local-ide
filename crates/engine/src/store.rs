use std::sync::{Arc, Mutex, PoisonError};

use session_store::{SessionStore, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum StoreAccessError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the storage task stopped: {0}")]
    Task(#[from] tokio::task::JoinError),
}

// Runs each synchronous store call on the blocking pool, so the async workers stay free
#[derive(Clone)]
pub struct Store {
    inner: Arc<Mutex<SessionStore>>,
}

impl Store {
    pub fn new(store: SessionStore) -> Self {
        Self {
            inner: Arc::new(Mutex::new(store)),
        }
    }

    pub async fn call<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut SessionStore) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreAccessError> {
        let inner = self.inner.clone();
        let result = tokio::task::spawn_blocking(move || {
            operation(&mut inner.lock().unwrap_or_else(PoisonError::into_inner))
        })
        .await??;
        Ok(result)
    }
}
