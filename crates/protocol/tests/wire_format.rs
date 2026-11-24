use std::fmt::Debug;

use protocol::{
    ApiError, Attachment, ClientFrame, CompactionKind, ContextReport, CreateSession, DeltaChannel,
    EngineStatus, Event, EventBody, ForkSession, LineRange, Mode, ModelInfo, ModelProfile,
    RejectReason, RunOutcome, ServerFrame, Session, SessionDetail, SessionOrigin, ToolCall,
    ToolCodec, ToolName, UpdateSession, Usage, WorkspaceSnapshot,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

const SESSION_ID: &str = "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
const PARENT_ID: &str = "0193a1b2-0000-7000-8000-000000000001";

fn assert_wire_shape<T>(value: &T, json: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let expected: serde_json::Value = serde_json::from_str(json).expect("literal is valid JSON");
    let encoded = serde_json::to_value(value).expect("value encodes");
    assert_eq!(encoded, expected, "encoded shape");
    let decoded: T = serde_json::from_str(json).expect("literal decodes");
    assert_eq!(&decoded, value, "decoded value");
}

fn session(origin: SessionOrigin) -> Session {
    Session {
        id: SESSION_ID.to_owned(),
        title: "Fix the parser".to_owned(),
        workspace: "local-ide".to_owned(),
        model: "coder".to_owned(),
        mode: Mode::Agent,
        origin,
        created_at: 1_795_470_660_000,
        updated_at: 1_795_470_665_000,
    }
}

fn read_call() -> ToolCall {
    ToolCall {
        id: "call_1".to_owned(),
        name: "read".to_owned(),
        arguments: json!({ "path": "src/main.rs", "offset": 1 }),
    }
}

#[test]
fn session_with_root_origin() {
    assert_wire_shape(
        &session(SessionOrigin::Root),
        r#"{
            "id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "title": "Fix the parser",
            "workspace": "local-ide",
            "model": "coder",
            "mode": "agent",
            "origin": { "kind": "root" },
            "created_at": 1795470660000,
            "updated_at": 1795470665000
        }"#,
    );
}

#[test]
fn session_with_fork_origin() {
    assert_wire_shape(
        &session(SessionOrigin::Fork {
            parent: PARENT_ID.to_owned(),
            seq: 7,
        }),
        r#"{
            "id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "title": "Fix the parser",
            "workspace": "local-ide",
            "model": "coder",
            "mode": "agent",
            "origin": {
                "kind": "fork",
                "parent": "0193a1b2-0000-7000-8000-000000000001",
                "seq": 7
            },
            "created_at": 1795470660000,
            "updated_at": 1795470665000
        }"#,
    );
}

#[test]
fn session_with_delegate_origin() {
    assert_wire_shape(
        &session(SessionOrigin::Delegate {
            parent: PARENT_ID.to_owned(),
            call_id: "call_4".to_owned(),
        }),
        r#"{
            "id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "title": "Fix the parser",
            "workspace": "local-ide",
            "model": "coder",
            "mode": "agent",
            "origin": {
                "kind": "delegate",
                "parent": "0193a1b2-0000-7000-8000-000000000001",
                "call_id": "call_4"
            },
            "created_at": 1795470660000,
            "updated_at": 1795470665000
        }"#,
    );
}

#[test]
fn user_event() {
    assert_wire_shape(
        &Event {
            seq: 1,
            at: 1_795_470_660_000,
            body: EventBody::User {
                text: "Explain this function".to_owned(),
                attachments: vec![
                    Attachment {
                        path: "src/config.rs".to_owned(),
                        lines: Some(LineRange {
                            first: 10,
                            last: 24,
                        }),
                        content: "fn load() {}".to_owned(),
                    },
                    Attachment {
                        path: "README.md".to_owned(),
                        lines: None,
                        content: "# local-ide".to_owned(),
                    },
                ],
                workspace: Some(WorkspaceSnapshot {
                    name: "local-ide".to_owned(),
                    entries: vec!["crates/".to_owned(), "Cargo.toml".to_owned()],
                    active_file: None,
                }),
                system_rules: None,
                repo_rules: None,
            },
        },
        r##"{
            "seq": 1,
            "at": 1795470660000,
            "body": {
                "type": "user",
                "text": "Explain this function",
                "attachments": [
                    {
                        "path": "src/config.rs",
                        "lines": { "first": 10, "last": 24 },
                        "content": "fn load() {}"
                    },
                    { "path": "README.md", "lines": null, "content": "# local-ide" }
                ],
                "workspace": {
                    "name": "local-ide",
                    "entries": ["crates/", "Cargo.toml"],
                    "active_file": null
                },
                "system_rules": null,
                "repo_rules": null
            }
        }"##,
    );
}

#[test]
fn assistant_event_with_tool_call_and_usage() {
    assert_wire_shape(
        &Event {
            seq: 2,
            at: 1_795_470_661_000,
            body: EventBody::Assistant {
                text: "I read the file first.".to_owned(),
                reasoning: "The user wants an explanation.".to_owned(),
                tool_calls: vec![read_call()],
                model: "coder".to_owned(),
                usage: Some(Usage {
                    prompt_tokens: 812,
                    completion_tokens: 64,
                    tokens_per_second: Some(42.5),
                }),
            },
        },
        r#"{
            "seq": 2,
            "at": 1795470661000,
            "body": {
                "type": "assistant",
                "text": "I read the file first.",
                "reasoning": "The user wants an explanation.",
                "tool_calls": [
                    {
                        "id": "call_1",
                        "name": "read",
                        "arguments": { "path": "src/main.rs", "offset": 1 }
                    }
                ],
                "model": "coder",
                "usage": {
                    "prompt_tokens": 812,
                    "completion_tokens": 64,
                    "tokens_per_second": 42.5
                }
            }
        }"#,
    );
}

#[test]
fn usage_without_generation_speed() {
    assert_wire_shape(
        &Usage {
            prompt_tokens: 812,
            completion_tokens: 64,
            tokens_per_second: None,
        },
        r#"{ "prompt_tokens": 812, "completion_tokens": 64, "tokens_per_second": null }"#,
    );
}

#[test]
fn assistant_event_without_usage() {
    assert_wire_shape(
        &EventBody::Assistant {
            text: "Done.".to_owned(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
            model: "fast".to_owned(),
            usage: None,
        },
        r#"{
            "type": "assistant",
            "text": "Done.",
            "reasoning": "",
            "tool_calls": [],
            "model": "fast",
            "usage": null
        }"#,
    );
}

#[test]
fn tool_result_event() {
    assert_wire_shape(
        &Event {
            seq: 3,
            at: 1_795_470_662_000,
            body: EventBody::ToolResult {
                call_id: "call_1".to_owned(),
                name: "read".to_owned(),
                output: "error: file not found: src/main.rs".to_owned(),
                is_error: true,
            },
        },
        r#"{
            "seq": 3,
            "at": 1795470662000,
            "body": {
                "type": "tool_result",
                "call_id": "call_1",
                "name": "read",
                "output": "error: file not found: src/main.rs",
                "is_error": true
            }
        }"#,
    );
}

#[test]
fn compaction_event_with_elide() {
    assert_wire_shape(
        &Event {
            seq: 12,
            at: 1_795_470_663_000,
            body: EventBody::Compaction {
                through: 9,
                kind: CompactionKind::Elide,
            },
        },
        r#"{
            "seq": 12,
            "at": 1795470663000,
            "body": { "type": "compaction", "through": 9, "kind": { "kind": "elide" } }
        }"#,
    );
}

#[test]
fn compaction_event_with_summary() {
    assert_wire_shape(
        &EventBody::Compaction {
            through: 20,
            kind: CompactionKind::Summary {
                text: "The user renamed the loader.".to_owned(),
                model: "guide".to_owned(),
            },
        },
        r#"{
            "type": "compaction",
            "through": 20,
            "kind": {
                "kind": "summary",
                "text": "The user renamed the loader.",
                "model": "guide"
            }
        }"#,
    );
}

#[test]
fn model_switch_event() {
    assert_wire_shape(
        &Event {
            seq: 13,
            at: 1_795_470_664_000,
            body: EventBody::ModelSwitch {
                from: "guide".to_owned(),
                to: "coder".to_owned(),
            },
        },
        r#"{
            "seq": 13,
            "at": 1795470664000,
            "body": { "type": "model_switch", "from": "guide", "to": "coder" }
        }"#,
    );
}

#[test]
fn client_run_start_frame() {
    assert_wire_shape(
        &ClientFrame::RunStart {
            session_id: SESSION_ID.to_owned(),
            text: "Rename the config loader".to_owned(),
            attachments: vec![Attachment {
                path: "src/config.rs".to_owned(),
                lines: None,
                content: "fn load() {}".to_owned(),
            }],
            workspace: WorkspaceSnapshot {
                name: "local-ide".to_owned(),
                entries: vec!["crates/".to_owned(), "Cargo.toml".to_owned()],
                active_file: Some("src/config.rs".to_owned()),
            },
            capabilities: vec![
                ToolName::Read,
                ToolName::Glob,
                ToolName::Grep,
                ToolName::Edit,
                ToolName::Write,
            ],
            repo_rules: None,
        },
        r#"{
            "type": "run_start",
            "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "text": "Rename the config loader",
            "attachments": [
                { "path": "src/config.rs", "lines": null, "content": "fn load() {}" }
            ],
            "workspace": {
                "name": "local-ide",
                "entries": ["crates/", "Cargo.toml"],
                "active_file": "src/config.rs"
            },
            "capabilities": ["read", "glob", "grep", "edit", "write"],
            "repo_rules": null
        }"#,
    );
}

#[test]
fn client_run_cancel_frame() {
    assert_wire_shape(&ClientFrame::RunCancel, r#"{ "type": "run_cancel" }"#);
}

#[test]
fn client_tool_result_frame() {
    assert_wire_shape(
        &ClientFrame::ToolResult {
            call_id: "call_1".to_owned(),
            output: "wrote src/main.rs (3 lines)".to_owned(),
            is_error: false,
        },
        r#"{
            "type": "tool_result",
            "call_id": "call_1",
            "output": "wrote src/main.rs (3 lines)",
            "is_error": false
        }"#,
    );
}

#[test]
fn server_run_started_frame() {
    assert_wire_shape(
        &ServerFrame::RunStarted {
            session_id: SESSION_ID.to_owned(),
        },
        r#"{ "type": "run_started", "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b" }"#,
    );
}

#[test]
fn server_delta_frame_on_each_channel() {
    assert_wire_shape(
        &ServerFrame::Delta {
            session_id: SESSION_ID.to_owned(),
            channel: DeltaChannel::Text,
            text: "Hel".to_owned(),
        },
        r#"{
            "type": "delta",
            "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "channel": "text",
            "text": "Hel"
        }"#,
    );
    assert_wire_shape(
        &ServerFrame::Delta {
            session_id: SESSION_ID.to_owned(),
            channel: DeltaChannel::Reasoning,
            text: "First".to_owned(),
        },
        r#"{
            "type": "delta",
            "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "channel": "reasoning",
            "text": "First"
        }"#,
    );
}

#[test]
fn server_tool_call_frame() {
    assert_wire_shape(
        &ServerFrame::ToolCall {
            session_id: SESSION_ID.to_owned(),
            call: read_call(),
        },
        r#"{
            "type": "tool_call",
            "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "call": {
                "id": "call_1",
                "name": "read",
                "arguments": { "path": "src/main.rs", "offset": 1 }
            }
        }"#,
    );
}

#[test]
fn server_event_frame() {
    assert_wire_shape(
        &ServerFrame::Event {
            session_id: SESSION_ID.to_owned(),
            event: Event {
                seq: 1,
                at: 1_795_470_660_000,
                body: EventBody::User {
                    text: "Hello".to_owned(),
                    attachments: Vec::new(),
                    workspace: None,
                    system_rules: None,
                    repo_rules: None,
                },
            },
        },
        r#"{
            "type": "event",
            "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "event": {
                "seq": 1,
                "at": 1795470660000,
                "body": {
                    "type": "user",
                    "text": "Hello",
                    "attachments": [],
                    "workspace": null,
                    "system_rules": null,
                    "repo_rules": null
                }
            }
        }"#,
    );
}

#[test]
fn server_context_report_frame() {
    assert_wire_shape(
        &ServerFrame::ContextReport {
            session_id: SESSION_ID.to_owned(),
            model: "coder".to_owned(),
            report: ContextReport {
                budget: 24000,
                reserve: 2048,
                system: 310,
                tools: 180,
                history: 5200,
                total: 5690,
            },
        },
        r#"{
            "type": "context_report",
            "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "model": "coder",
            "report": {
                "budget": 24000,
                "reserve": 2048,
                "system": 310,
                "tools": 180,
                "history": 5200,
                "total": 5690
            }
        }"#,
    );
}

#[test]
fn server_session_changed_frame() {
    assert_wire_shape(
        &ServerFrame::SessionChanged {
            session: session(SessionOrigin::Root),
        },
        r#"{
            "type": "session_changed",
            "session": {
                "id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
                "title": "Fix the parser",
                "workspace": "local-ide",
                "model": "coder",
                "mode": "agent",
                "origin": { "kind": "root" },
                "created_at": 1795470660000,
                "updated_at": 1795470665000
            }
        }"#,
    );
}

#[test]
fn server_run_finished_frame_with_each_outcome() {
    let outcomes = [
        (RunOutcome::Complete, r#"{ "kind": "complete" }"#),
        (RunOutcome::Cancelled, r#"{ "kind": "cancelled" }"#),
        (RunOutcome::StepLimit, r#"{ "kind": "step_limit" }"#),
        (
            RunOutcome::Failed {
                message: "inference server timeout".to_owned(),
            },
            r#"{ "kind": "failed", "message": "inference server timeout" }"#,
        ),
    ];
    for (outcome, json) in outcomes {
        assert_wire_shape(&outcome, json);
    }

    assert_wire_shape(
        &ServerFrame::RunFinished {
            session_id: SESSION_ID.to_owned(),
            outcome: RunOutcome::Complete,
        },
        r#"{
            "type": "run_finished",
            "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "outcome": { "kind": "complete" }
        }"#,
    );
}

#[test]
fn server_rejected_frame_with_each_reason() {
    assert_wire_shape(
        &ServerFrame::Rejected {
            reason: RejectReason::Busy {
                session_id: SESSION_ID.to_owned(),
            },
        },
        r#"{
            "type": "rejected",
            "reason": { "kind": "busy", "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b" }
        }"#,
    );
    assert_wire_shape(
        &ServerFrame::Rejected {
            reason: RejectReason::InvalidRequest {
                message: "unknown session".to_owned(),
            },
        },
        r#"{
            "type": "rejected",
            "reason": { "kind": "invalid_request", "message": "unknown session" }
        }"#,
    );
}

#[test]
fn model_profile_with_sampling_overrides() {
    assert_wire_shape(
        &ModelProfile {
            id: "coder".to_owned(),
            label: "Coder".to_owned(),
            role: "General coding".to_owned(),
            context_budget: 24000,
            reply_reserve: 8192,
            tool_codec: ToolCodec::Native,
            temperature: Some(0.5),
            top_p: None,
            delegate_target: true,
            chars_per_token: 3.25,
        },
        r#"{
            "id": "coder",
            "label": "Coder",
            "role": "General coding",
            "context_budget": 24000,
            "reply_reserve": 8192,
            "tool_codec": "native",
            "temperature": 0.5,
            "top_p": null,
            "delegate_target": true,
            "chars_per_token": 3.25
        }"#,
    );
}

#[test]
fn unprofiled_model_info() {
    assert_wire_shape(
        &ModelInfo {
            profile: ModelProfile::unprofiled("new-model"),
            available: true,
        },
        r#"{
            "profile": {
                "id": "new-model",
                "label": "new-model",
                "role": "",
                "context_budget": 16384,
                "reply_reserve": 2048,
                "tool_codec": "text",
                "temperature": null,
                "top_p": null,
                "delegate_target": false,
                "chars_per_token": 3.5
            },
            "available": true
        }"#,
    );
}

#[test]
fn model_profile_decodes_without_optional_fields() {
    let profile: ModelProfile = serde_json::from_str(
        r#"{
            "id": "fast",
            "label": "Fast",
            "role": "Small fast model",
            "context_budget": 16000,
            "reply_reserve": 2048,
            "tool_codec": "text",
            "delegate_target": true
        }"#,
    )
    .expect("profile decodes");

    assert_eq!(profile.temperature, None);
    assert_eq!(profile.top_p, None);
    assert_eq!(profile.chars_per_token, 3.5);
}

#[test]
fn engine_status() {
    assert_wire_shape(
        &EngineStatus {
            version: "0.1.0".to_owned(),
            active_run: Some(SESSION_ID.to_owned()),
        },
        r#"{ "version": "0.1.0", "active_run": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b" }"#,
    );
    assert_wire_shape(
        &EngineStatus {
            version: "0.1.0".to_owned(),
            active_run: None,
        },
        r#"{ "version": "0.1.0", "active_run": null }"#,
    );
}

#[test]
fn create_session_body() {
    assert_wire_shape(
        &CreateSession {
            workspace: "local-ide".to_owned(),
            model: "guide".to_owned(),
            mode: Mode::Ask,
        },
        r#"{ "workspace": "local-ide", "model": "guide", "mode": "ask" }"#,
    );
}

#[test]
fn update_session_body() {
    assert_wire_shape(
        &UpdateSession {
            title: Some("Parser fix".to_owned()),
            model: None,
            mode: Some(Mode::Ask),
        },
        r#"{ "title": "Parser fix", "model": null, "mode": "ask" }"#,
    );
}

#[test]
fn update_session_body_decodes_absent_fields_as_none() {
    let update: UpdateSession =
        serde_json::from_str(r#"{ "model": "coder" }"#).expect("partial body decodes");

    assert_eq!(
        update,
        UpdateSession {
            title: None,
            model: Some("coder".to_owned()),
            mode: None,
        }
    );
}

#[test]
fn fork_session_body() {
    assert_wire_shape(&ForkSession { seq: 7 }, r#"{ "seq": 7 }"#);
}

#[test]
fn session_detail() {
    assert_wire_shape(
        &SessionDetail {
            session: session(SessionOrigin::Root),
            events: vec![Event {
                seq: 1,
                at: 1_795_470_660_000,
                body: EventBody::User {
                    text: "Hello".to_owned(),
                    attachments: Vec::new(),
                    workspace: Some(WorkspaceSnapshot {
                        name: "local-ide".to_owned(),
                        entries: vec!["src/".to_owned()],
                        active_file: Some("src/main.rs".to_owned()),
                    }),
                    system_rules: None,
                    repo_rules: None,
                },
            }],
            context: ContextReport {
                budget: 24000,
                reserve: 8192,
                system: 310,
                tools: 180,
                history: 40,
                total: 530,
            },
        },
        r#"{
            "session": {
                "id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
                "title": "Fix the parser",
                "workspace": "local-ide",
                "model": "coder",
                "mode": "agent",
                "origin": { "kind": "root" },
                "created_at": 1795470660000,
                "updated_at": 1795470665000
            },
            "events": [
                {
                    "seq": 1,
                    "at": 1795470660000,
                    "body": {
                        "type": "user",
                        "text": "Hello",
                        "attachments": [],
                        "workspace": {
                            "name": "local-ide",
                            "entries": ["src/"],
                            "active_file": "src/main.rs"
                        },
                        "system_rules": null,
                        "repo_rules": null
                    }
                }
            ],
            "context": {
                "budget": 24000,
                "reserve": 8192,
                "system": 310,
                "tools": 180,
                "history": 40,
                "total": 530
            }
        }"#,
    );
}

#[test]
fn api_error() {
    assert_wire_shape(
        &ApiError {
            message: "session has the active run".to_owned(),
        },
        r#"{ "message": "session has the active run" }"#,
    );
}

#[test]
fn frame_with_an_unknown_type_tag_fails_to_decode() {
    let decoded = serde_json::from_str::<ClientFrame>(r#"{ "type": "run_pause" }"#);

    assert!(decoded.is_err());
}

#[test]
fn a_stored_user_event_can_omit_the_rules_fields() {
    let event: Event = serde_json::from_str(
        r#"{
            "seq": 1,
            "at": 1,
            "body": { "type": "user", "text": "Hi", "attachments": [], "workspace": null }
        }"#,
    )
    .expect("an older user event decodes");

    assert_eq!(
        event.body,
        EventBody::User {
            text: "Hi".to_owned(),
            attachments: Vec::new(),
            workspace: None,
            system_rules: None,
            repo_rules: None,
        }
    );
}

#[test]
fn a_run_start_can_omit_repo_rules() {
    let frame: ClientFrame = serde_json::from_str(
        r#"{
            "type": "run_start",
            "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b",
            "text": "Hi",
            "attachments": [],
            "workspace": { "name": "demo", "entries": [], "active_file": null },
            "capabilities": []
        }"#,
    )
    .expect("an older run start decodes");

    assert_eq!(
        frame,
        ClientFrame::RunStart {
            session_id: SESSION_ID.to_owned(),
            text: "Hi".to_owned(),
            attachments: Vec::new(),
            workspace: WorkspaceSnapshot {
                name: "demo".to_owned(),
                entries: Vec::new(),
                active_file: None,
            },
            capabilities: Vec::new(),
            repo_rules: None,
        }
    );
}
