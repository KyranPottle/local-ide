use std::pin::pin;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use inference::{ChatChunk, ChatRequest, InferenceClient, InferenceError};
use prompt::{ChatToolCall, StreamSplitter};
use protocol::{ModelProfile, Usage};

use crate::run::{REPLY_TOO_LONG, ReplyBudget, chat_tool_calls};

pub(super) enum ExchangeError {
    // The server answered and refused the request, which counts as a failed probe
    Rejected(String),
    // The server is not usable, so the probe cannot run
    Unavailable(String),
}

impl ExchangeError {
    pub fn into_detail(self) -> String {
        match self {
            Self::Rejected(detail) | Self::Unavailable(detail) => detail,
        }
    }
}

impl From<InferenceError> for ExchangeError {
    fn from(error: InferenceError) -> Self {
        let detail = error.to_string();
        match error {
            InferenceError::HttpStatus { .. }
            | InferenceError::MalformedJson(_)
            | InferenceError::Server(_)
            | InferenceError::LimitExceeded { .. }
            | InferenceError::Truncated => Self::Rejected(detail),
            InferenceError::InvalidBaseUrl(_)
            | InferenceError::InvalidApiKey
            | InferenceError::Connect(_)
            | InferenceError::Transport(_)
            | InferenceError::IdleTimeout(_)
            | InferenceError::Unauthorized(_) => Self::Unavailable(detail),
        }
    }
}

pub(super) struct Reply {
    pub text: String,
    pub native_calls: Vec<ChatToolCall>,
    pub usage: Option<Usage>,
    // Time from the request to the first stream item, which includes a model load
    pub first_byte: Duration,
}

pub(super) async fn exchange(
    client: &InferenceClient,
    profile: &ModelProfile,
    request: ChatRequest,
    call_id_prefix: &str,
) -> Result<Reply, ExchangeError> {
    let started = Instant::now();
    let mut first_item = None;
    let mut budget = ReplyBudget::new(profile);
    let mut splitter = StreamSplitter::new(profile.tool_codec);
    let mut stream = pin!(client.chat(request));
    while let Some(item) = stream.next().await {
        let first_byte = *first_item.get_or_insert_with(|| started.elapsed());
        let delta = match item? {
            ChatChunk::Text(delta) => {
                splitter.push(&delta);
                delta
            }
            ChatChunk::Reasoning(delta) => delta,
            ChatChunk::Finished(completion) => {
                return Ok(Reply {
                    text: splitter.finish().text,
                    native_calls: chat_tool_calls(completion.tool_calls, call_id_prefix),
                    usage: completion.usage,
                    first_byte,
                });
            }
        };
        if !budget.admits(&delta) {
            return Err(ExchangeError::Rejected(REPLY_TOO_LONG.to_owned()));
        }
    }
    Err(ExchangeError::Rejected(
        "the inference stream ended without a completion".to_owned(),
    ))
}
