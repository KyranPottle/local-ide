mod access;
mod assets;
pub mod calibration;
mod catalog;
pub mod config;
mod connection;
mod routes;
mod run;
mod run_lock;
mod store;

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::Router;
use axum::middleware::{from_fn, from_fn_with_state};
use inference::{InferenceClient, InferenceError};
use session_store::{SessionStore, StoreError};
use tokio::net::TcpListener;

use crate::access::AccessPolicy;
use crate::catalog::ModelCatalog;
use crate::config::Config;
use crate::run_lock::RunLock;
use crate::store::Store;

const STORE_FILE: &str = "sessions.sqlite";

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("cannot create the data directory {0}: {1}")]
    DataDir(std::path::PathBuf, std::io::Error),
    #[error("cannot open the session store: {0}")]
    Store(#[from] StoreError),
    #[error("cannot set up the inference client: {0}")]
    Inference(#[from] InferenceError),
    #[error("the HTTP server failed: {0}")]
    Serve(#[from] std::io::Error),
}

struct RunSettings {
    step_limit: u32,
    tool_output_limit: usize,
    tool_timeout: Duration,
    system_rules: Option<String>,
}

struct Engine {
    settings: RunSettings,
    store: Store,
    catalog: ModelCatalog,
    inference: InferenceClient,
    run_lock: Arc<RunLock>,
    // Correction of the token estimate per model id, kept in memory only
    token_scales: Mutex<HashMap<String, f32>>,
}

impl Engine {
    fn token_scale(&self, model: &str) -> f32 {
        self.token_scales
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(model)
            .copied()
            .unwrap_or(1.0)
    }

    fn set_token_scale(&self, model: &str, scale: f32) {
        self.token_scales
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(model.to_owned(), scale);
    }
}

pub async fn serve(
    config: Config,
    listener: TcpListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), StartError> {
    std::fs::create_dir_all(&config.data_dir)
        .map_err(|error| StartError::DataDir(config.data_dir.clone(), error))?;
    let session_store = SessionStore::open(&config.data_dir.join(STORE_FILE), unix_millis)?;
    let engine = Arc::new(Engine {
        settings: RunSettings {
            step_limit: config.step_limit,
            tool_output_limit: config.tool_output_limit,
            tool_timeout: config.tool_timeout,
            system_rules: config.system_rules,
        },
        store: Store::new(session_store),
        catalog: ModelCatalog::new(config.models),
        inference: InferenceClient::new(config.inference)?,
        run_lock: Arc::default(),
        token_scales: Mutex::default(),
    });
    let policy = Arc::new(AccessPolicy {
        allowed_peers: config.allowed_peers,
        public_origin: config.public_origin,
    });
    let application = Router::new()
        .merge(routes::router(engine))
        .merge(assets::router(&config.assets_dir))
        .layer(from_fn_with_state(policy, access::enforce))
        .layer(from_fn(access::add_security_headers));
    axum::serve(
        listener,
        application.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await?;
    Ok(())
}

fn unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}
