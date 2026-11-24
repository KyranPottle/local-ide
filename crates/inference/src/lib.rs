mod decoder;
mod line_buffer;

use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use futures_util::Stream;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use reqwest::{RequestBuilder, Response, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::decoder::{EventDecoder, Outcome};

pub use crate::decoder::{MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_CALLS};
pub use crate::line_buffer::MAX_LINE_BYTES;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const MAX_MODEL_LIST_BYTES: usize = 4 * 1024 * 1024;
const MALFORMED_EXCERPT_CHARS: usize = 200;
const STATUS_BODY_CHARS: usize = 500;
// Four bytes per character is the UTF-8 worst case for the kept excerpt
const STATUS_BODY_BYTES: usize = STATUS_BODY_CHARS * 4;
const REDACTION: &str = "[redacted]";

pub struct InferenceSettings {
    pub base_url: String,
    pub api_key: Option<String>,
    // Longest wait for the next bytes from the server, which includes a model load
    pub idle_timeout: Duration,
}

impl fmt::Debug for InferenceSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InferenceSettings")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| REDACTION))
            .field("idle_timeout", &self.idle_timeout)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
}

#[derive(Serialize)]
struct RequestBody {
    #[serde(flatten)]
    request: ChatRequest,
    stream: bool,
    stream_options: StreamOptions,
    cache_prompt: bool,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatChunk {
    Text(String),
    Reasoning(String),
    Finished(ChatCompletion),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatCompletion {
    pub tool_calls: Vec<NativeToolCall>,
    pub finish: FinishReason,
    pub usage: Option<protocol::Usage>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NativeToolCall {
    pub id: String,
    pub name: String,
    // Concatenated argument fragments as the server sent them
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    Other(String),
}

impl FinishReason {
    fn from_wire(reason: String) -> Self {
        match reason.as_str() {
            "stop" => Self::Stop,
            "length" => Self::Length,
            "tool_calls" => Self::ToolCalls,
            _ => Self::Other(reason),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("invalid inference base URL: {0}")]
    InvalidBaseUrl(&'static str),
    #[error("API key contains characters that an HTTP header cannot carry")]
    InvalidApiKey,
    #[error("cannot connect to the inference server: {0}")]
    Connect(#[source] reqwest::Error),
    #[error("HTTP transport error: {0}")]
    Transport(#[source] reqwest::Error),
    #[error("inference server sent no bytes for {0:?}")]
    IdleTimeout(Duration),
    #[error("inference server rejected the API key with HTTP {0}")]
    Unauthorized(u16),
    #[error("inference server returned HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },
    #[error("stream ended before completion")]
    Truncated,
    #[error("inference server sent invalid JSON: {0}")]
    MalformedJson(String),
    #[error("inference server error: {0}")]
    Server(String),
    #[error("inference server response exceeds the {limit} limit of {max}")]
    LimitExceeded { limit: &'static str, max: usize },
}

impl InferenceError {
    fn from_transport(error: reqwest::Error) -> Self {
        if error.is_connect() {
            return Self::Connect(error);
        }
        Self::Transport(error)
    }

    // A hostile or misconfigured server can echo the Authorization header back in text it controls
    fn redacted(self, secret: Option<&str>) -> Self {
        let Some(secret) = secret else {
            return self;
        };
        let scrub = |text: String| text.replace(secret, REDACTION);
        match self {
            Self::HttpStatus { status, body } => Self::HttpStatus {
                status,
                body: scrub(body),
            },
            Self::MalformedJson(text) => Self::MalformedJson(scrub(text)),
            Self::Server(message) => Self::Server(scrub(message)),
            Self::InvalidBaseUrl(_)
            | Self::InvalidApiKey
            | Self::Connect(_)
            | Self::Transport(_)
            | Self::IdleTimeout(_)
            | Self::Unauthorized(_)
            | Self::Truncated
            | Self::LimitExceeded { .. } => self,
        }
    }
}

fn excerpt(bytes: &[u8], max_chars: usize) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .take(max_chars)
        .collect()
}

async fn within<T>(
    idle: Duration,
    operation: impl Future<Output = Result<T, reqwest::Error>>,
) -> Result<T, InferenceError> {
    tokio::time::timeout(idle, operation)
        .await
        .map_err(|_| InferenceError::IdleTimeout(idle))?
        .map_err(InferenceError::from_transport)
}

async fn read_prefix(
    mut response: Response,
    max_bytes: usize,
    idle: Duration,
) -> Result<Vec<u8>, InferenceError> {
    let mut body = Vec::new();
    while body.len() < max_bytes {
        let Some(chunk) = within(idle, response.chunk()).await? else {
            break;
        };
        body.extend_from_slice(&chunk);
    }
    body.truncate(max_bytes);
    Ok(body)
}

#[derive(Deserialize)]
struct ModelList {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

pub struct InferenceClient {
    http: reqwest::Client,
    base_url: String,
    secret: Option<Arc<str>>,
    idle_timeout: Duration,
}

impl fmt::Debug for InferenceClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InferenceClient")
            .field("base_url", &self.base_url)
            .field("idle_timeout", &self.idle_timeout)
            .finish_non_exhaustive()
    }
}

impl InferenceClient {
    pub fn new(settings: InferenceSettings) -> Result<Self, InferenceError> {
        let url = Url::parse(&settings.base_url)
            .map_err(|_| InferenceError::InvalidBaseUrl("unparsable"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(InferenceError::InvalidBaseUrl(
                "scheme is not http or https",
            ));
        }
        let mut headers = HeaderMap::new();
        if let Some(key) = &settings.api_key {
            let mut value = HeaderValue::from_str(&format!("Bearer {key}"))
                .map_err(|_| InferenceError::InvalidApiKey)?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .default_headers(headers)
            .build()
            .map_err(InferenceError::Transport)?;
        Ok(Self {
            http,
            base_url: settings.base_url.trim_end_matches('/').to_owned(),
            secret: settings.api_key.map(Arc::from),
            idle_timeout: settings.idle_timeout,
        })
    }

    pub async fn model_ids(&self) -> Result<Vec<String>, InferenceError> {
        self.fetch_model_ids()
            .await
            .map_err(|error| error.redacted(self.secret.as_deref()))
    }

    async fn fetch_model_ids(&self) -> Result<Vec<String>, InferenceError> {
        let request = self.http.get(format!("{}/models", self.base_url));
        let response = send(request, self.idle_timeout).await?;
        let body = read_prefix(response, MAX_MODEL_LIST_BYTES, self.idle_timeout).await?;
        let list: ModelList = serde_json::from_slice(&body)
            .map_err(|_| InferenceError::MalformedJson(excerpt(&body, MALFORMED_EXCERPT_CHARS)))?;
        Ok(list.data.into_iter().map(|model| model.id).collect())
    }

    // Dropping the stream closes the connection, which makes llama.cpp stop generation
    pub fn chat(
        &self,
        request: ChatRequest,
    ) -> impl Stream<Item = Result<ChatChunk, InferenceError>> + Send + use<> {
        let body = RequestBody {
            request,
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
            cache_prompt: true,
        };
        let state = ChatStream {
            phase: Phase::Connect(
                self.http
                    .post(format!("{}/chat/completions", self.base_url))
                    .json(&body),
            ),
            pending: VecDeque::new(),
            idle_timeout: self.idle_timeout,
            secret: self.secret.clone(),
        };
        futures_util::stream::unfold(state, |mut state| async move {
            let item = state.next_item().await?;
            Some((item, state))
        })
    }
}

async fn send(request: RequestBuilder, idle: Duration) -> Result<Response, InferenceError> {
    let response = within(idle, request.send()).await?;
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(InferenceError::Unauthorized(status.as_u16()));
    }
    let body = read_prefix(response, STATUS_BODY_BYTES, idle).await?;
    Err(InferenceError::HttpStatus {
        status: status.as_u16(),
        body: excerpt(&body, STATUS_BODY_CHARS),
    })
}

enum Phase {
    Connect(RequestBuilder),
    Receive(Response, Box<EventDecoder>),
    Ended,
}

struct ChatStream {
    phase: Phase,
    pending: VecDeque<Result<ChatChunk, InferenceError>>,
    idle_timeout: Duration,
    secret: Option<Arc<str>>,
}

impl ChatStream {
    async fn next_item(&mut self) -> Option<Result<ChatChunk, InferenceError>> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Some(item.map_err(|error| error.redacted(self.secret.as_deref())));
            }
            match std::mem::replace(&mut self.phase, Phase::Ended) {
                Phase::Ended => return None,
                Phase::Connect(request) => match send(request, self.idle_timeout).await {
                    Ok(response) => self.phase = Phase::Receive(response, Box::default()),
                    Err(error) => self.pending.push_back(Err(error)),
                },
                Phase::Receive(mut response, mut decoder) => {
                    match within(self.idle_timeout, response.chunk()).await {
                        Ok(Some(bytes)) => match decoder.feed(&bytes, &mut self.pending) {
                            Ok(Outcome::Pending) => self.phase = Phase::Receive(response, decoder),
                            Ok(Outcome::Complete) => {}
                            Err(error) => self.pending.push_back(Err(error)),
                        },
                        Ok(None) => self.pending.push_back(Err(InferenceError::Truncated)),
                        Err(error) => self.pending.push_back(Err(error)),
                    }
                }
            }
        }
    }
}
