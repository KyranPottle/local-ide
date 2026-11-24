use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientFrame, RejectReason, ServerFrame};
use tokio::sync::{mpsc, watch};

use crate::Engine;
use crate::run::{self, RunLink, ToolReplies, ToolReply};

// A tool result can be large before the engine caps it
const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const OUTBOX_CAPACITY: usize = 64;

pub async fn upgrade(State(engine): State<Arc<Engine>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| serve_connection(engine, socket))
}

async fn serve_connection(engine: Arc<Engine>, socket: WebSocket) {
    let (mut sink, mut inbox) = socket.split();
    let (frames, mut outbox) = mpsc::channel::<ServerFrame>(OUTBOX_CAPACITY);
    let writer = tokio::spawn(async move {
        while let Some(frame) = outbox.recv().await {
            let Ok(text) = serde_json::to_string(&frame) else {
                continue;
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
    let tool_replies = ToolReplies::default();
    let mut active_cancel: Option<watch::Sender<bool>> = None;
    while let Some(Ok(message)) = inbox.next().await {
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => break,
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Binary(_) => {
                reject(&frames, "frames must be JSON text".to_owned());
                continue;
            }
        };
        let frame = match serde_json::from_str::<ClientFrame>(text.as_str()) {
            Ok(frame) => frame,
            Err(error) => {
                reject(&frames, format!("unparsable frame: {error}"));
                continue;
            }
        };
        match frame {
            ClientFrame::RunStart {
                session_id,
                text,
                attachments,
                workspace,
                capabilities,
                repo_rules,
            } => {
                let guard = match engine.run_lock.acquire(&session_id) {
                    Ok(guard) => guard,
                    Err(active) => {
                        let reason = RejectReason::Busy { session_id: active };
                        let _ = frames.try_send(ServerFrame::Rejected { reason });
                        continue;
                    }
                };
                let request = run::prepare(
                    &engine,
                    &session_id,
                    text,
                    attachments,
                    workspace,
                    capabilities,
                    repo_rules,
                )
                .await;
                let request = match request {
                    Ok(request) => request,
                    Err(message) => {
                        reject(&frames, message);
                        continue;
                    }
                };
                let (cancel, cancelled) = watch::channel(false);
                let link = RunLink {
                    frames: frames.clone(),
                    cancelled,
                    tool_replies: tool_replies.clone(),
                };
                tokio::spawn(run::execute(engine.clone(), guard, link, request));
                active_cancel = Some(cancel);
            }
            ClientFrame::RunCancel => {
                if let Some(cancel) = &active_cancel {
                    cancel.send_replace(true);
                }
            }
            ClientFrame::ToolResult {
                call_id,
                output,
                is_error,
            } => tool_replies.resolve(&call_id, ToolReply { output, is_error }),
        }
    }
    if let Some(cancel) = active_cancel {
        cancel.send_replace(true);
    }
    drop(frames);
    writer.abort();
}

// A client that does not read its frames loses rejections instead of stalling this loop
fn reject(frames: &mpsc::Sender<ServerFrame>, message: String) {
    let reason = RejectReason::InvalidRequest { message };
    let _ = frames.try_send(ServerFrame::Rejected { reason });
}
