use std::time::{Duration, Instant};

use inference::InferenceClient;
use protocol::{ModelInfo, ModelProfile};
use tokio::sync::Mutex;

const LIST_LIFETIME: Duration = Duration::from_secs(10);

struct ServerList {
    fetched_at: Instant,
    // None when the inference server did not answer
    ids: Option<Vec<String>>,
}

pub struct ModelCatalog {
    profiles: Vec<ModelProfile>,
    // An async lock makes concurrent callers share one fetch, and a failed fetch is cached as
    // well so that an unreachable server costs one connect timeout per lifetime
    server_list: Mutex<Option<ServerList>>,
}

impl ModelCatalog {
    pub fn new(profiles: Vec<ModelProfile>) -> Self {
        Self {
            profiles,
            server_list: Mutex::new(None),
        }
    }

    pub fn profile(&self, id: &str) -> ModelProfile {
        self.profiles
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
            .unwrap_or_else(|| ModelProfile::unprofiled(id))
    }

    pub async fn models(&self, client: &InferenceClient) -> Vec<ModelInfo> {
        let server_ids = self.server_ids(client).await.unwrap_or_default();
        let configured = self.profiles.iter().map(|profile| ModelInfo {
            available: server_ids.contains(&profile.id),
            profile: profile.clone(),
        });
        let unprofiled = server_ids
            .iter()
            .filter(|id| self.profiles.iter().all(|profile| profile.id != **id))
            .map(|id| ModelInfo {
                profile: ModelProfile::unprofiled(id),
                available: true,
            });
        configured.chain(unprofiled).collect()
    }

    pub async fn delegate_targets(
        &self,
        client: &InferenceClient,
        current: &str,
    ) -> Vec<ModelProfile> {
        let server_ids = self.server_ids(client).await.unwrap_or_default();
        self.profiles
            .iter()
            .filter(|profile| {
                profile.delegate_target && profile.id != current && server_ids.contains(&profile.id)
            })
            .cloned()
            .collect()
    }

    // A profile with delegate_target that the server lists, including the current model
    pub async fn delegate_target(
        &self,
        client: &InferenceClient,
        id: &str,
    ) -> Option<ModelProfile> {
        let server_ids = self.server_ids(client).await?;
        self.profiles
            .iter()
            .find(|profile| profile.id == id && profile.delegate_target)
            .filter(|profile| server_ids.contains(&profile.id))
            .cloned()
    }

    async fn server_ids(&self, client: &InferenceClient) -> Option<Vec<String>> {
        let mut cached = self.server_list.lock().await;
        if let Some(list) = cached
            .as_ref()
            .filter(|list| list.fetched_at.elapsed() < LIST_LIFETIME)
        {
            return list.ids.clone();
        }
        let ids = match client.model_ids().await {
            Ok(ids) => Some(ids),
            Err(error) => {
                tracing::warn!(%error, "the inference server did not list its models");
                None
            }
        };
        *cached = Some(ServerList {
            fetched_at: Instant::now(),
            ids: ids.clone(),
        });
        ids
    }
}
