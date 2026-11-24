use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use engine::config::Config;
use futures_util::{SinkExt, Stream, StreamExt, stream};
use protocol::{
    ClientFrame, Event, EventBody, RunOutcome, ServerFrame, Session, ToolName, WorkspaceSnapshot,
};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

pub const API_KEY: &str = "sk-secret-test-key-4711";
pub const ORIGIN: &str = "http://ide.test:5173";
const FRAME_WAIT: Duration = Duration::from_secs(10);

pub enum Script {
    Stream(Vec<String>),
    // Sends the chunks and then holds the stream open, the sender fires when the engine drops it
    Hold(Vec<String>, oneshot::Sender<()>),
    Status(StatusCode, String),
}

pub struct RecordedRequest {
    pub authorization: Option<String>,
    pub body: Value,
}

#[derive(Default)]
struct MockState {
    scripts: Mutex<VecDeque<Script>>,
    requests: Mutex<Vec<RecordedRequest>>,
    model_ids: Mutex<Option<Vec<String>>>,
    in_flight: AtomicUsize,
    most_in_flight: AtomicUsize,
}

struct InFlight(Arc<MockState>);

impl InFlight {
    fn begin(state: &Arc<MockState>) -> Self {
        let count = state.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        state.most_in_flight.fetch_max(count, Ordering::SeqCst);
        Self(state.clone())
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
pub struct MockInference {
    pub base_url: String,
    state: Arc<MockState>,
}

struct DropSignal(Option<oneshot::Sender<()>>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

impl MockInference {
    pub async fn start(model_ids: &[&str]) -> Self {
        let state = Arc::new(MockState::default());
        *state.model_ids.lock().unwrap() =
            Some(model_ids.iter().map(|id| (*id).to_owned()).collect());
        let router = Router::new()
            .route("/v1/models", get(list_models))
            .route("/v1/chat/completions", post(chat))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            base_url: format!("http://{address}/v1"),
            state,
        }
    }

    pub fn push(&self, script: Script) {
        self.state.scripts.lock().unwrap().push_back(script);
    }

    pub fn push_text(&self, text: &str) {
        self.push(Script::Stream(text_reply(text)));
    }

    pub fn requests(&self) -> Vec<Value> {
        let requests = self.state.requests.lock().unwrap();
        requests
            .iter()
            .map(|request| request.body.clone())
            .collect()
    }

    pub fn authorizations(&self) -> Vec<Option<String>> {
        let requests = self.state.requests.lock().unwrap();
        requests
            .iter()
            .map(|request| request.authorization.clone())
            .collect()
    }

    pub fn most_in_flight(&self) -> usize {
        self.state.most_in_flight.load(Ordering::SeqCst)
    }

    pub fn fail_model_list(&self) {
        *self.state.model_ids.lock().unwrap() = None;
    }

    pub fn models_requested(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|body| body["model"].as_str().unwrap().to_owned())
            .collect()
    }
}

async fn list_models(State(state): State<Arc<MockState>>) -> Response {
    match state.model_ids.lock().unwrap().clone() {
        Some(ids) => {
            let data: Vec<Value> = ids.iter().map(|id| json!({ "id": id })).collect();
            axum::Json(json!({ "object": "list", "data": data })).into_response()
        }
        None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn chat(State(state): State<Arc<MockState>>, headers: HeaderMap, body: Bytes) -> Response {
    let in_flight = InFlight::begin(&state);
    let authorization = headers
        .get("authorization")
        .map(|value| value.to_str().unwrap().to_owned());
    state.requests.lock().unwrap().push(RecordedRequest {
        authorization,
        body: serde_json::from_slice(&body).unwrap(),
    });
    let script = state.scripts.lock().unwrap().pop_front();
    match script {
        Some(Script::Stream(chunks)) => {
            // The request ends before its last chunk leaves, so a client that waits for the
            // reply can never overlap with it
            let last = chunks.len().saturating_sub(1);
            let mut held = Some(in_flight);
            let marked = stream::iter(chunks).enumerate().map(move |(index, chunk)| {
                if index == last {
                    held.take();
                }
                chunk
            });
            sse(marked.boxed())
        }
        Some(Script::Hold(chunks, dropped)) => {
            let signal = DropSignal(Some(dropped));
            let held = stream::iter(chunks)
                .chain(stream::pending())
                .map(move |chunk| {
                    let _ = (&signal, &in_flight);
                    chunk
                });
            sse(held.boxed())
        }
        Some(Script::Status(status, text)) => (status, text).into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, "no scripted reply").into_response(),
    }
}

fn sse(chunks: impl Stream<Item = String> + Send + 'static) -> Response {
    let body = chunks.then(|chunk| async move {
        tokio::time::sleep(Duration::from_millis(1)).await;
        Ok::<_, Infallible>(Bytes::from(chunk))
    });
    (
        [("content-type", "text/event-stream")],
        Body::from_stream(body),
    )
        .into_response()
}

pub fn workspace_release_version() -> String {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    let mut in_workspace_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line == "[workspace.package]" {
            in_workspace_package = true;
            continue;
        }
        if in_workspace_package && line.starts_with('[') {
            break;
        }
        if in_workspace_package && line.starts_with("version") {
            return line
                .split_once('=')
                .unwrap()
                .1
                .trim()
                .trim_matches('"')
                .to_owned();
        }
    }
    panic!(
        "workspace.package version not found in {}",
        manifest.display()
    );
}

pub fn sse_data_event(value: Value) -> String {
    format!("data: {value}\n\n")
}

pub fn content_chunk(text: &str) -> String {
    sse_data_event(json!({ "choices": [{ "delta": { "content": text } }] }))
}

// A text reply with usage and a speed, as llama-server sends them
pub fn usage_reply(text: &str, prompt_tokens: u32, tokens_per_second: f64) -> Vec<String> {
    vec![
        content_chunk(text),
        sse_data_event(json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] })),
        sse_data_event(json!({
            "choices": [],
            "usage": { "prompt_tokens": prompt_tokens, "completion_tokens": 1 },
            "timings": { "predicted_per_second": tokens_per_second }
        })),
        "data: [DONE]\n\n".to_owned(),
    ]
}

pub fn finish_chunks(reason: &str) -> Vec<String> {
    vec![
        sse_data_event(json!({ "choices": [{ "delta": {}, "finish_reason": reason }] })),
        "data: [DONE]\n\n".to_owned(),
    ]
}

pub fn text_reply(text: &str) -> Vec<String> {
    let middle = text
        .char_indices()
        .nth(text.chars().count() / 2)
        .map_or(0, |(index, _)| index);
    let mut chunks = vec![
        content_chunk(&text[..middle]),
        content_chunk(&text[middle..]),
    ];
    chunks.extend(finish_chunks("stop"));
    chunks
}

pub fn text_call(name: &str, arguments: Value) -> String {
    format!(
        "<tool_call>\n{}\n</tool_call>",
        json!({ "name": name, "arguments": arguments })
    )
}

pub fn native_call_reply(id: &str, name: &str, arguments: Value) -> Vec<String> {
    let mut chunks = vec![sse_data_event(
        json!({ "choices": [{ "delta": { "tool_calls": [{
        "index": 0,
        "id": id,
        "function": { "name": name, "arguments": arguments.to_string() }
    }] } }] }),
    )];
    chunks.extend(finish_chunks("tool_calls"));
    chunks
}

pub struct Settings {
    pub allowed_peer: &'static str,
    pub step_limit: u32,
    pub tool_timeout_seconds: u64,
    pub system_rules: Option<&'static str>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            allowed_peer: "127.0.0.1",
            step_limit: 25,
            tool_timeout_seconds: 30,
            system_rules: None,
        }
    }
}

pub const SERVER_MODELS: [&str; 6] = ["alpha", "beta", "native", "tiny", "capped", "extra"];

const MODEL_TABLES: &str = r#"
[[model]]
id = "alpha"
label = "Alpha"
role = "General model"
context_budget = 16000
reply_reserve = 1000
tool_codec = "text"
delegate_target = true

[[model]]
id = "beta"
label = "Beta"
role = "Second model"
context_budget = 16000
reply_reserve = 1000
tool_codec = "text"
delegate_target = true

[[model]]
id = "native"
label = "Native"
role = "Native tool caller"
context_budget = 16000
reply_reserve = 1000
tool_codec = "native"
delegate_target = false

[[model]]
id = "tiny"
label = "Tiny"
role = "Tiny context"
context_budget = 1500
reply_reserve = 200
tool_codec = "text"
delegate_target = false

[[model]]
id = "gamma"
label = "Gamma"
role = "Offline model"
context_budget = 16000
reply_reserve = 1000
tool_codec = "text"
delegate_target = true

[[model]]
id = "capped"
label = "Capped"
role = "Reply size cap"
context_budget = 1500
reply_reserve = 4
tool_codec = "text"
delegate_target = false
"#;

pub fn config_text(
    root: &std::path::Path,
    listen: &str,
    base_url: &str,
    settings: &Settings,
) -> String {
    let assets = root.join("web");
    std::fs::create_dir_all(assets.join("assets")).unwrap();
    std::fs::write(
        assets.join("index.html"),
        "<!doctype html><title>local-ide</title>",
    )
    .unwrap();
    let system_rules = match settings.system_rules {
        Some(text) => format!("system_rules = \"{text}\"\n"),
        None => String::new(),
    };
    format!(
        r#"
listen = "{listen}"
public_origin = "{ORIGIN}"
allowed_peers = ["{peer}"]
data_dir = "{data}"
assets_dir = "{assets}"
step_limit = {steps}
tool_output_limit = 2000
tool_timeout_seconds = {tool_timeout}
{system_rules}
[inference]
base_url = "{base_url}"
api_key_env = "TEST_KEY"
idle_timeout_seconds = 5
{MODEL_TABLES}"#,
        peer = settings.allowed_peer,
        data = root.join("var").display(),
        assets = assets.display(),
        steps = settings.step_limit,
        tool_timeout = settings.tool_timeout_seconds,
    )
}

pub fn parse_config(text: &str) -> Config {
    Config::parse(text, |name| {
        (name == "TEST_KEY").then(|| API_KEY.to_owned())
    })
    .unwrap()
}

pub struct Harness {
    pub address: std::net::SocketAddr,
    pub mock: MockInference,
    pub http: reqwest::Client,
    pub root: PathBuf,
    _shutdown: oneshot::Sender<()>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub fn temporary_directory() -> PathBuf {
    let root = std::env::temp_dir().join(format!("local-ide-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

impl Harness {
    pub async fn start() -> Self {
        Self::start_with(Settings::default()).await
    }

    pub async fn start_with(settings: Settings) -> Self {
        let mock = MockInference::start(&SERVER_MODELS).await;
        let root = temporary_directory();
        let config = parse_config(&config_text(
            &root,
            "127.0.0.1:0",
            &mock.base_url,
            &settings,
        ));
        let assets = root.join("web");
        std::fs::write(assets.join("assets/app.js"), "console.log(1)").unwrap();
        std::fs::write(assets.join("assets/app.js.gz"), "gzip bytes").unwrap();
        std::fs::write(root.join("secret.txt"), "outside the assets").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (shutdown, stop) = oneshot::channel::<()>();
        tokio::spawn(async move {
            engine::serve(config, listener, async move {
                let _ = stop.await;
            })
            .await
            .unwrap();
        });
        Self {
            address,
            mock,
            http: reqwest::Client::new(),
            root,
            _shutdown: shutdown,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    pub async fn create_session(&self, model: &str) -> Session {
        let response = self
            .http
            .post(self.url("/api/sessions"))
            .header("origin", ORIGIN)
            .json(&json!({ "workspace": "demo", "model": model, "mode": "agent" }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        response.json().await.unwrap()
    }

    pub async fn patch(&self, id: &str, body: Value) -> reqwest::Response {
        self.http
            .patch(self.url(&format!("/api/sessions/{id}")))
            .header("origin", ORIGIN)
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    pub async fn delete(&self, id: &str) -> reqwest::Response {
        self.http
            .delete(self.url(&format!("/api/sessions/{id}")))
            .header("origin", ORIGIN)
            .send()
            .await
            .unwrap()
    }

    pub async fn get_json(&self, path: &str) -> (u16, Value) {
        let response = self.http.get(self.url(path)).send().await.unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap())
    }

    pub async fn events(&self, id: &str) -> Vec<Event> {
        let (status, detail) = self.get_json(&format!("/api/sessions/{id}")).await;
        assert_eq!(status, 200);
        serde_json::from_value(detail["events"].clone()).unwrap()
    }

    pub async fn active_run(&self) -> Value {
        self.get_json("/api/status").await.1["active_run"].clone()
    }

    // Polls because the run task releases the lock just after its last frame
    pub async fn wait_until_idle(&self) {
        for _ in 0..200 {
            if self.active_run().await.is_null() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the run lock stayed held");
    }
}

pub fn workspace(entries: &[&str]) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        name: "demo".to_owned(),
        entries: entries.iter().map(|entry| (*entry).to_owned()).collect(),
        active_file: None,
    }
}

pub fn run_start(session_id: &str, text: &str) -> ClientFrame {
    run_start_in(session_id, text, workspace(&["src/"]))
}

pub fn run_start_in(session_id: &str, text: &str, workspace: WorkspaceSnapshot) -> ClientFrame {
    ClientFrame::RunStart {
        session_id: session_id.to_owned(),
        text: text.to_owned(),
        attachments: Vec::new(),
        workspace,
        capabilities: ToolName::ALL
            .into_iter()
            .filter(|tool| tool.is_workspace_tool())
            .collect(),
        repo_rules: None,
    }
}

pub fn run_start_with_rules(session_id: &str, text: &str, repo_rules: Option<&str>) -> ClientFrame {
    let mut frame = run_start(session_id, text);
    if let ClientFrame::RunStart {
        repo_rules: slot, ..
    } = &mut frame
    {
        *slot = repo_rules.map(str::to_owned);
    }
    frame
}

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct Browser {
    socket: Socket,
    pub tool_outputs: HashMap<String, String>,
}

pub async fn connect_with_origin(
    harness: &Harness,
    origin: Option<&str>,
) -> Result<Socket, tokio_tungstenite::tungstenite::Error> {
    let mut request = format!("ws://{}/api/ws", harness.address)
        .into_client_request()
        .unwrap();
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert("origin", origin.parse().unwrap());
    }
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}

impl Browser {
    pub async fn connect(harness: &Harness) -> Self {
        Self {
            socket: connect_with_origin(harness, Some(ORIGIN)).await.unwrap(),
            tool_outputs: HashMap::new(),
        }
    }

    pub fn answer(mut self, tool: &str, output: &str) -> Self {
        self.tool_outputs.insert(tool.to_owned(), output.to_owned());
        self
    }

    pub async fn send(&mut self, frame: &ClientFrame) {
        let text = serde_json::to_string(frame).unwrap();
        self.socket.send(Message::text(text)).await.unwrap();
    }

    pub async fn send_text(&mut self, text: &str) {
        self.socket.send(Message::text(text)).await.unwrap();
    }

    pub async fn next_frame(&mut self) -> ServerFrame {
        loop {
            let message = tokio::time::timeout(FRAME_WAIT, self.socket.next())
                .await
                .expect("a frame arrives in time")
                .expect("the socket stays open")
                .unwrap();
            if let Message::Text(text) = message {
                return serde_json::from_str(text.as_str()).unwrap();
            }
        }
    }

    pub async fn read_until(&mut self, stop: impl Fn(&ServerFrame) -> bool) -> Vec<ServerFrame> {
        let mut frames = Vec::new();
        loop {
            let frame = self.next_frame().await;
            if let ServerFrame::ToolCall { call, .. } = &frame
                && let Some(output) = self.tool_outputs.get(&call.name)
            {
                let reply = ClientFrame::ToolResult {
                    call_id: call.id.clone(),
                    output: output.clone(),
                    is_error: false,
                };
                self.send(&reply).await;
            }
            let done = stop(&frame);
            frames.push(frame);
            if done {
                return frames;
            }
        }
    }

    pub async fn run(&mut self, session_id: &str, text: &str) -> Vec<ServerFrame> {
        self.send(&run_start(session_id, text)).await;
        self.finish(session_id).await
    }

    pub async fn finish(&mut self, session_id: &str) -> Vec<ServerFrame> {
        let session_id = session_id.to_owned();
        self.read_until(move |frame| {
            matches!(frame, ServerFrame::RunFinished { session_id: id, .. } if *id == session_id)
        })
        .await
    }

    pub async fn close(mut self) {
        self.socket.close(None).await.unwrap();
    }
}

pub fn outcome(frames: &[ServerFrame]) -> RunOutcome {
    match frames.last() {
        Some(ServerFrame::RunFinished { outcome, .. }) => outcome.clone(),
        other => panic!("the last frame is not RunFinished: {other:?}"),
    }
}

// One label per frame, with consecutive deltas folded into one
pub fn frame_labels(frames: &[ServerFrame]) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for frame in frames {
        let label = match frame {
            ServerFrame::RunStarted { .. } => "run_started".to_owned(),
            ServerFrame::Delta { .. } => "delta".to_owned(),
            ServerFrame::ToolCall { call, .. } => format!("tool_call {}", call.name),
            ServerFrame::Event { event, .. } => {
                format!("event {} {}", event.seq, body_label(&event.body))
            }
            ServerFrame::ContextReport { model, .. } => format!("context_report {model}"),
            ServerFrame::SessionChanged { .. } => "session_changed".to_owned(),
            ServerFrame::RunFinished { outcome, .. } => format!("run_finished {outcome:?}"),
            ServerFrame::Rejected { reason } => format!("rejected {reason:?}"),
        };
        if label == "delta" && labels.last().is_some_and(|last| last == "delta") {
            continue;
        }
        labels.push(label);
    }
    labels
}

pub fn body_label(body: &EventBody) -> &'static str {
    match body {
        EventBody::User { .. } => "user",
        EventBody::Assistant { .. } => "assistant",
        EventBody::ToolResult { .. } => "tool_result",
        EventBody::Compaction { .. } => "compaction",
        EventBody::ModelSwitch { .. } => "model_switch",
    }
}

pub fn event_labels(events: &[Event]) -> Vec<(u32, &'static str)> {
    events
        .iter()
        .map(|event| (event.seq, body_label(&event.body)))
        .collect()
}

pub fn tool_results(events: &[Event]) -> Vec<(String, String, bool)> {
    events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::ToolResult {
                name,
                output,
                is_error,
                ..
            } => Some((name.clone(), output.clone(), *is_error)),
            _ => None,
        })
        .collect()
}

pub fn last_message_content(request: &Value) -> String {
    let messages = request["messages"].as_array().unwrap();
    messages.last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_owned()
}

pub fn all_content(request: &Value) -> String {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["content"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}
