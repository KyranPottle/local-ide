use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::{Stream, StreamExt, stream};
use inference::{
    ChatChunk, ChatCompletion, ChatRequest, FinishReason, InferenceClient, InferenceError,
    InferenceSettings, NativeToolCall,
};
use protocol::Usage;
use serde_json::{Value, json};
use tokio::sync::oneshot;

const API_KEY: &str = "sk-test-0123456789";

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{address}/v1")
}

fn client(base_url: String, idle_timeout: Duration) -> InferenceClient {
    InferenceClient::new(InferenceSettings {
        base_url,
        api_key: Some(API_KEY.to_owned()),
        idle_timeout,
    })
    .unwrap()
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "example-a".to_owned(),
        messages: json!([{"role": "user", "content": "hi"}]),
        tools: None,
        max_tokens: 256,
        temperature: None,
        top_p: None,
    }
}

fn sse_response(chunks: Vec<Vec<u8>>) -> Response {
    let body = stream::iter(chunks).then(|chunk| async move {
        tokio::time::sleep(Duration::from_millis(1)).await;
        Ok::<_, Infallible>(Bytes::from(chunk))
    });
    streaming_response(body)
}

fn streaming_response(
    body: impl Stream<Item = Result<Bytes, Infallible>> + Send + 'static,
) -> Response {
    (
        [("content-type", "text/event-stream")],
        Body::from_stream(body),
    )
        .into_response()
}

async fn chat_server(chunks: Vec<&'static str>) -> String {
    let chunks: Vec<Vec<u8>> = chunks.into_iter().map(|c| c.as_bytes().to_vec()).collect();
    let router = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let chunks = chunks.clone();
            async move { sse_response(chunks) }
        }),
    );
    serve(router).await
}

async fn run_chat(base_url: String) -> Vec<Result<ChatChunk, InferenceError>> {
    client(base_url, Duration::from_secs(5))
        .chat(request())
        .collect()
        .await
}

fn assert_secret_absent(error: &InferenceError) {
    assert!(!error.to_string().contains(API_KEY));
    assert!(!format!("{error:?}").contains(API_KEY));
}

fn single_error(items: &[Result<ChatChunk, InferenceError>]) -> &InferenceError {
    let errors: Vec<&InferenceError> = items.iter().filter_map(|i| i.as_ref().err()).collect();
    assert_eq!(errors.len(), 1);
    assert!(items.last().unwrap().is_err());
    assert_secret_absent(errors[0]);
    errors[0]
}

fn chunks(items: Vec<Result<ChatChunk, InferenceError>>) -> Vec<ChatChunk> {
    items.into_iter().map(Result::unwrap).collect()
}

#[tokio::test]
async fn sends_bearer_header_and_exact_body() {
    let captured: Arc<Mutex<Option<(String, Value)>>> = Arc::default();
    let sink = captured.clone();
    let router = Router::new().route(
        "/v1/chat/completions",
        post(move |headers: HeaderMap, body: Bytes| async move {
            let authorization = headers["authorization"].to_str().unwrap().to_owned();
            let body: Value = serde_json::from_slice(&body).unwrap();
            *sink.lock().unwrap() = Some((authorization, body));
            sse_response(vec![
                b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
                    .to_vec(),
            ])
        }),
    );
    let base_url = serve(router).await;
    let items = run_chat(base_url.clone()).await;
    assert_eq!(items.len(), 1);
    let (authorization, body) = captured.lock().unwrap().take().unwrap();
    assert_eq!(authorization, "Bearer sk-test-0123456789");
    assert_eq!(
        body,
        json!({
            "model": "example-a",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 256,
            "stream": true,
            "stream_options": {"include_usage": true},
            "cache_prompt": true
        })
    );

    let mut full = request();
    full.tools = Some(json!([{"type": "function", "function": {"name": "read_file"}}]));
    full.temperature = Some(0.5);
    full.top_p = Some(0.25);
    let items: Vec<_> = client(base_url, Duration::from_secs(5))
        .chat(full)
        .collect()
        .await;
    assert_eq!(items.len(), 1);
    let (_, body) = captured.lock().unwrap().take().unwrap();
    assert_eq!(
        body,
        json!({
            "model": "example-a",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function", "function": {"name": "read_file"}}],
            "max_tokens": 256,
            "temperature": 0.5,
            "top_p": 0.25,
            "stream": true,
            "stream_options": {"include_usage": true},
            "cache_prompt": true
        })
    );
}

#[tokio::test]
async fn lists_model_ids_in_server_order() {
    let router = Router::new().route(
        "/v1/models",
        get(|| async {
            axum::Json(json!({"object": "list", "data": [
                {"id": "example-a", "object": "model"},
                {"id": "example-b", "object": "model"},
                {"id": "example-c", "object": "model"}
            ]}))
        }),
    );
    let ids = client(serve(router).await, Duration::from_secs(5))
        .model_ids()
        .await
        .unwrap();
    assert_eq!(ids, vec!["example-a", "example-b", "example-c"]);
}

#[tokio::test]
async fn streams_plain_text_and_skips_empty_deltas() {
    let base_url = chat_server(vec![
        "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":null}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
        "data: [DONE]\n\n",
    ])
    .await;
    assert_eq!(
        chunks(run_chat(base_url).await),
        vec![
            ChatChunk::Text("Hello".to_owned()),
            ChatChunk::Text(" world".to_owned()),
            ChatChunk::Finished(ChatCompletion {
                tool_calls: vec![],
                finish: FinishReason::Length,
                usage: None,
            }),
        ]
    );
}

#[tokio::test]
async fn streams_reasoning_deltas() {
    let base_url = chat_server(vec![
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"Think\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"ing\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"Answer\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    ])
    .await;
    assert_eq!(
        chunks(run_chat(base_url).await),
        vec![
            ChatChunk::Reasoning("Think".to_owned()),
            ChatChunk::Reasoning("ing".to_owned()),
            ChatChunk::Text("Answer".to_owned()),
            ChatChunk::Finished(ChatCompletion {
                tool_calls: vec![],
                finish: FinishReason::Stop,
                usage: None,
            }),
        ]
    );
}

#[tokio::test]
async fn collects_fragmented_tool_calls_by_index() {
    let base_url = chat_server(vec![
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"list_dir\",\"arguments\":\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"pa\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"th\\\":\\\"a.rs\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    ])
    .await;
    assert_eq!(
        chunks(run_chat(base_url).await),
        vec![ChatChunk::Finished(ChatCompletion {
            tool_calls: vec![
                NativeToolCall {
                    id: "call_a".to_owned(),
                    name: "read_file".to_owned(),
                    arguments: "{\"path\":\"a.rs\"}".to_owned(),
                },
                NativeToolCall {
                    id: "call_b".to_owned(),
                    name: "list_dir".to_owned(),
                    arguments: "{}".to_owned(),
                },
            ],
            finish: FinishReason::ToolCalls,
            usage: None,
        })]
    );
}

#[tokio::test]
async fn maps_usage_with_timings() {
    let base_url = chat_server(vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":120,\"completion_tokens\":8,\"total_tokens\":128},\"timings\":{\"predicted_per_second\":42.5}}\n\n",
        "data: [DONE]\n\n",
    ])
    .await;
    assert_eq!(
        chunks(run_chat(base_url).await),
        vec![
            ChatChunk::Text("ok".to_owned()),
            ChatChunk::Finished(ChatCompletion {
                tool_calls: vec![],
                finish: FinishReason::Stop,
                usage: Some(Usage {
                    prompt_tokens: 120,
                    completion_tokens: 8,
                    tokens_per_second: Some(42.5),
                }),
            }),
        ]
    );
}

#[tokio::test]
async fn maps_usage_without_timings() {
    let base_url = chat_server(vec![
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    ])
    .await;
    assert_eq!(
        chunks(run_chat(base_url).await),
        vec![ChatChunk::Finished(ChatCompletion {
            tool_calls: vec![],
            finish: FinishReason::Stop,
            usage: Some(Usage {
                prompt_tokens: 7,
                completion_tokens: 3,
                tokens_per_second: None,
            }),
        })]
    );
}

#[tokio::test]
async fn ignores_bytes_after_done() {
    let base_url = chat_server(vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"late\"}}]}\n\n",
    ])
    .await;
    assert_eq!(
        chunks(run_chat(base_url).await),
        vec![
            ChatChunk::Text("a".to_owned()),
            ChatChunk::Finished(ChatCompletion {
                tool_calls: vec![],
                finish: FinishReason::Stop,
                usage: None,
            }),
        ]
    );
}

#[tokio::test]
async fn done_without_finish_reason_is_truncated() {
    let base_url = chat_server(vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
        "data: [DONE]\n\n",
    ])
    .await;
    let items = run_chat(base_url).await;
    assert!(matches!(single_error(&items), InferenceError::Truncated));
}

#[tokio::test]
async fn reassembles_a_line_split_at_every_byte() {
    let event = "data: {\"choices\":[{\"delta\":{\"content\":\"Grüße 日本\"}}]}\n\n";
    let tail =
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let shared: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
    let source = shared.clone();
    let router = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let chunks = source.lock().unwrap().clone();
            async move { sse_response(chunks) }
        }),
    );
    let base_url = serve(router).await;
    let bytes = event.as_bytes();
    for split in 1..bytes.len() {
        *shared.lock().unwrap() = vec![
            bytes[..split].to_vec(),
            bytes[split..].to_vec(),
            tail.as_bytes().to_vec(),
        ];
        assert_eq!(
            chunks(run_chat(base_url.clone()).await),
            vec![
                ChatChunk::Text("Grüße 日本".to_owned()),
                ChatChunk::Finished(ChatCompletion {
                    tool_calls: vec![],
                    finish: FinishReason::Stop,
                    usage: None,
                }),
            ],
            "split at byte {split}"
        );
    }
}

#[tokio::test]
async fn accepts_crlf_line_ends_and_comments() {
    let base_url = chat_server(vec![
        ": keep-alive\r\n\r\n",
        "event: message\r\ndata:{\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\r\n\r\n",
        ":\r\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\r\n",
        "data: [DONE]\r\n\r\n",
    ])
    .await;
    assert_eq!(
        chunks(run_chat(base_url).await),
        vec![
            ChatChunk::Text("x".to_owned()),
            ChatChunk::Finished(ChatCompletion {
                tool_calls: vec![],
                finish: FinishReason::Stop,
                usage: None,
            }),
        ]
    );
}

#[tokio::test]
async fn maps_401_to_unauthorized() {
    let router = Router::new()
        .route(
            "/v1/chat/completions",
            post(|| async { (StatusCode::UNAUTHORIZED, "invalid key") }),
        )
        .route(
            "/v1/models",
            get(|| async { (StatusCode::UNAUTHORIZED, "invalid key") }),
        );
    let base_url = serve(router).await;
    let items = run_chat(base_url.clone()).await;
    assert!(matches!(
        single_error(&items),
        InferenceError::Unauthorized(401)
    ));
    let error = client(base_url, Duration::from_secs(5))
        .model_ids()
        .await
        .unwrap_err();
    assert_secret_absent(&error);
    assert!(matches!(error, InferenceError::Unauthorized(401)));
}

#[tokio::test]
async fn cuts_long_500_body_and_redacts_echoed_key() {
    let router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            let body = format!("Bearer {API_KEY} {}", "é".repeat(2000));
            (StatusCode::INTERNAL_SERVER_ERROR, body)
        }),
    );
    let items = run_chat(serve(router).await).await;
    let InferenceError::HttpStatus { status, body } = single_error(&items) else {
        panic!("expected an HTTP status error");
    };
    assert_eq!(*status, 500);
    let expected = format!("Bearer [redacted] {}", "é".repeat(500 - 26));
    assert_eq!(body, &expected);
}

#[tokio::test]
async fn maps_in_stream_error_object() {
    let base_url = chat_server(vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
        "data: {\"error\":{\"code\":500,\"message\":\"context size exceeded\",\"type\":\"server_error\"}}\n\n",
    ])
    .await;
    let items = run_chat(base_url).await;
    assert_eq!(items[0].as_ref().unwrap(), &ChatChunk::Text("a".to_owned()));
    let InferenceError::Server(message) = single_error(&items) else {
        panic!("expected a server error");
    };
    assert_eq!(message, "context size exceeded");
}

#[tokio::test]
async fn reports_invalid_json_excerpt() {
    let payload = format!("{{\"choices\": {}", "x".repeat(400));
    let event: &'static str = Box::leak(format!("data: {payload}\n\n").into_boxed_str());
    let items = run_chat(chat_server(vec![event]).await).await;
    let InferenceError::MalformedJson(text) = single_error(&items) else {
        panic!("expected a JSON error");
    };
    assert_eq!(text, &format!("{{\"choices\": {}", "x".repeat(188)));
}

#[tokio::test]
async fn connection_closed_mid_stream_is_truncated() {
    let base_url = chat_server(vec![
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
    ])
    .await;
    let items = run_chat(base_url).await;
    assert_eq!(items[0].as_ref().unwrap(), &ChatChunk::Text("a".to_owned()));
    assert!(matches!(single_error(&items), InferenceError::Truncated));
}

#[tokio::test]
async fn idle_timeout_fires_when_server_stalls() {
    let router = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            let first = stream::once(async {
                Ok(Bytes::from_static(
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n",
                ))
            });
            streaming_response(first.chain(stream::pending()))
        }),
    );
    let base_url = serve(router).await;
    let items: Vec<_> = client(base_url, Duration::from_millis(200))
        .chat(request())
        .collect()
        .await;
    assert_eq!(items[0].as_ref().unwrap(), &ChatChunk::Text("a".to_owned()));
    assert!(matches!(
        single_error(&items),
        InferenceError::IdleTimeout(timeout) if *timeout == Duration::from_millis(200)
    ));
}

struct DisconnectSignal(Option<oneshot::Sender<()>>);

impl Drop for DisconnectSignal {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[tokio::test]
async fn dropping_the_stream_closes_the_connection() {
    let (sender, receiver) = oneshot::channel();
    let signal = Arc::new(Mutex::new(Some(sender)));
    let router = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let signal = DisconnectSignal(signal.lock().unwrap().take());
            async move {
                let body = stream::unfold(signal, |signal| async move {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    let chunk = Bytes::from_static(
                        b"data: {\"choices\":[{\"delta\":{\"content\":\"t\"}}]}\n\n",
                    );
                    Some((Ok(chunk), signal))
                });
                streaming_response(body)
            }
        }),
    );
    let base_url = serve(router).await;
    let mut stream = Box::pin(client(base_url, Duration::from_secs(5)).chat(request()));
    assert_eq!(
        stream.next().await.unwrap().unwrap(),
        ChatChunk::Text("t".to_owned())
    );
    drop(stream);
    tokio::time::timeout(Duration::from_secs(2), receiver)
        .await
        .expect("server did not observe the disconnect")
        .unwrap();
}

#[tokio::test]
async fn rejects_invalid_base_urls() {
    for base_url in [
        "ftp://192.0.2.10/v1",
        "not a url",
        "http://:8080/v1",
        "file:///tmp",
    ] {
        let result = InferenceClient::new(InferenceSettings {
            base_url: base_url.to_owned(),
            api_key: Some(API_KEY.to_owned()),
            idle_timeout: Duration::from_secs(1),
        });
        let error = result.unwrap_err();
        assert_secret_absent(&error);
        assert!(
            matches!(error, InferenceError::InvalidBaseUrl(_)),
            "{base_url}"
        );
    }
}

#[tokio::test]
async fn reports_connect_failure() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let error = client(format!("http://{address}/v1"), Duration::from_secs(5))
        .model_ids()
        .await
        .unwrap_err();
    assert_secret_absent(&error);
    assert!(matches!(error, InferenceError::Connect(_)));
}

#[test]
fn debug_output_hides_the_api_key() {
    let settings = InferenceSettings {
        base_url: "http://127.0.0.1:8080/v1".to_owned(),
        api_key: Some(API_KEY.to_owned()),
        idle_timeout: Duration::from_secs(60),
    };
    let settings_debug = format!("{settings:?}");
    assert!(!settings_debug.contains(API_KEY));
    let client_debug = format!("{:?}", InferenceClient::new(settings).unwrap());
    assert!(!client_debug.contains(API_KEY));
    assert_eq!(
        client_debug,
        "InferenceClient { base_url: \"http://127.0.0.1:8080/v1\", idle_timeout: 60s, .. }"
    );
}
