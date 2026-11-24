use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ToolCodec {
    // OpenAI tools request field and tool_calls response field
    Native,
    // The model writes <tool_call> blocks in its text and the engine parses them
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelProfile {
    /// Exact llama-swap model name
    pub id: String,
    pub label: String,
    /// One line on the strength of the model, shown to the user and to a delegating model
    pub role: String,
    /// Prompt token ceiling that the engine enforces, not the server n_ctx
    pub context_budget: u32,
    /// Sent as max_tokens
    pub reply_reserve: u32,
    pub tool_codec: ToolCodec,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    /// A delegating model may select this model
    pub delegate_target: bool,
    /// Seed of the token estimator
    #[serde(default = "default_chars_per_token")]
    pub chars_per_token: f32,
}

const fn default_chars_per_token() -> f32 {
    3.5
}

impl ModelProfile {
    // Conservative profile for a model that the configuration does not list
    pub fn unprofiled(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            label: id.to_owned(),
            role: String::new(),
            context_budget: 16384,
            reply_reserve: 2048,
            tool_codec: ToolCodec::Text,
            temperature: None,
            top_p: None,
            delegate_target: false,
            chars_per_token: default_chars_per_token(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModelInfo {
    pub profile: ModelProfile,
    /// The inference server lists this model
    pub available: bool,
}
