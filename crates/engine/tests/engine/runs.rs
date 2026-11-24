use std::time::Duration;

use protocol::{
    CompactionKind, Event, EventBody, RULES_CHARACTER_LIMIT, RejectReason, RunOutcome, ServerFrame,
    SessionOrigin, ToolCall,
};
use serde_json::json;
use tokio::sync::oneshot;

use crate::support::*;

#[tokio::test]
async fn full_run_with_a_read_call_in_the_text_codec() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness.mock.push_text(&format!(
        "I will read it.\n{}",
        text_call("read", json!({ "path": "src/main.rs" }))
    ));
    harness.mock.push_text("The file defines main.");
    let mut browser = Browser::connect(&harness)
        .await
        .answer("read", "1|fn main() {}");

    let frames = browser.run(&session.id, "Explain main").await;

    assert_eq!(
        frame_labels(&frames),
        [
            "run_started",
            "event 1 user",
            "session_changed",
            "context_report alpha",
            "delta",
            "event 2 assistant",
            "tool_call read",
            "event 3 tool_result",
            "context_report alpha",
            "delta",
            "event 4 assistant",
            "run_finished Complete",
        ]
    );
    let events = harness.events(&session.id).await;
    assert_eq!(
        event_labels(&events),
        [
            (1, "user"),
            (2, "assistant"),
            (3, "tool_result"),
            (4, "assistant")
        ]
    );
    let EventBody::Assistant {
        text,
        tool_calls,
        model,
        ..
    } = &events[1].body
    else {
        panic!("event 2 is not an assistant event");
    };
    assert_eq!(text, "I will read it.");
    assert_eq!(model, "alpha");
    assert_eq!(
        tool_calls,
        &[ToolCall {
            id: "call_2_0".to_owned(),
            name: "read".to_owned(),
            arguments: json!({ "path": "src/main.rs" }),
        }]
    );
    assert_eq!(
        tool_results(&events),
        [("read".to_owned(), "1|fn main() {}".to_owned(), false)]
    );
    let requests = harness.mock.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        last_message_content(&requests[1]),
        "<tool_response>\n1|fn main() {}\n</tool_response>"
    );
    assert_eq!(requests[0]["max_tokens"], 1000);
    assert!(requests[0].get("tools").is_none());
}

#[tokio::test]
async fn full_run_in_the_native_codec() {
    let harness = Harness::start().await;
    let session = harness.create_session("native").await;
    harness.mock.push(Script::Stream(native_call_reply(
        "call_a",
        "read",
        json!({ "path": "a.rs" }),
    )));
    harness.mock.push_text("a.rs is empty.");
    let mut browser = Browser::connect(&harness).await.answer("read", "1|");

    let frames = browser.run(&session.id, "Look at a.rs").await;

    assert_eq!(outcome(&frames), RunOutcome::Complete);
    let events = harness.events(&session.id).await;
    assert_eq!(
        event_labels(&events),
        [
            (1, "user"),
            (2, "assistant"),
            (3, "tool_result"),
            (4, "assistant")
        ]
    );
    let requests = harness.mock.requests();
    let tool_names: Vec<&str> = requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tool_names,
        ["read", "glob", "grep", "edit", "write", "delegate"]
    );
    let last = requests[1]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        last,
        json!({ "role": "tool", "tool_call_id": "call_a", "content": "1|" })
    );
}

#[tokio::test]
async fn full_run_with_native_call_arguments_in_fragments() {
    let harness = Harness::start().await;
    let session = harness.create_session("native").await;
    let fragment = |arguments: &str| {
        sse_data_event(json!({ "choices": [{ "delta": { "tool_calls": [{
            "index": 0,
            "function": { "arguments": arguments }
        }] } }] }))
    };
    let mut chunks = vec![
        sse_data_event(json!({ "choices": [{ "delta": { "tool_calls": [{
            "index": 0,
            "id": "call_b",
            "type": "function",
            "function": { "name": "read", "arguments": "" }
        }] } }] })),
        fragment("{\"pa"),
        fragment("th\": \"src/"),
        fragment("lib.rs\"}"),
    ];
    chunks.extend(finish_chunks("tool_calls"));
    harness.mock.push(Script::Stream(chunks));
    harness.mock.push_text("lib.rs holds one line.");
    let mut browser = Browser::connect(&harness).await.answer("read", "1|mod a;");

    let frames = browser.run(&session.id, "Read src/lib.rs").await;

    assert_eq!(outcome(&frames), RunOutcome::Complete);
    let events = harness.events(&session.id).await;
    assert_eq!(
        event_labels(&events),
        [
            (1, "user"),
            (2, "assistant"),
            (3, "tool_result"),
            (4, "assistant")
        ]
    );
    let EventBody::Assistant { tool_calls, .. } = &events[1].body else {
        panic!("event 2 is not an assistant event");
    };
    assert_eq!(
        tool_calls,
        &[ToolCall {
            id: "call_b".to_owned(),
            name: "read".to_owned(),
            arguments: json!({ "path": "src/lib.rs" }),
        }]
    );
    let EventBody::Assistant { text, .. } = &events[3].body else {
        panic!("event 4 is not an assistant event");
    };
    assert_eq!(text, "lib.rs holds one line.");
    let requests = harness.mock.requests();
    assert_eq!(requests.len(), 2);
    let last = requests[1]["messages"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        last,
        json!({ "role": "tool", "tool_call_id": "call_b", "content": "1|mod a;" })
    );
}

#[tokio::test]
async fn a_second_connection_is_busy_during_a_run_and_runs_after_it() {
    let harness = Harness::start().await;
    let first = harness.create_session("alpha").await;
    let second = harness.create_session("alpha").await;
    let (dropped, _observed) = oneshot::channel();
    harness
        .mock
        .push(Script::Hold(vec![content_chunk("Working")], dropped));
    let mut owner = Browser::connect(&harness).await;
    owner.send(&run_start(&first.id, "Long task")).await;
    owner
        .read_until(|frame| matches!(frame, ServerFrame::Delta { .. }))
        .await;

    let mut other = Browser::connect(&harness).await;
    other.send(&run_start(&second.id, "Other task")).await;
    assert_eq!(
        other.next_frame().await,
        ServerFrame::Rejected {
            reason: RejectReason::Busy {
                session_id: first.id.clone()
            }
        }
    );

    owner.send(&protocol::ClientFrame::RunCancel).await;
    assert_eq!(
        outcome(&owner.finish(&first.id).await),
        RunOutcome::Cancelled
    );
    harness.wait_until_idle().await;
    harness.mock.push_text("Done.");
    let frames = other.run(&second.id, "Other task").await;
    assert_eq!(outcome(&frames), RunOutcome::Complete);
}

#[tokio::test]
async fn cancel_during_a_stream_stores_the_partial_text_and_drops_the_request() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    let (dropped, observed) = oneshot::channel();
    harness
        .mock
        .push(Script::Hold(vec![content_chunk("Partial answer")], dropped));
    let mut browser = Browser::connect(&harness).await;
    browser.send(&run_start(&session.id, "Start")).await;
    browser
        .read_until(|frame| matches!(frame, ServerFrame::Delta { .. }))
        .await;

    browser.send(&protocol::ClientFrame::RunCancel).await;
    let frames = browser.finish(&session.id).await;

    assert_eq!(
        frame_labels(&frames),
        ["event 2 assistant", "run_finished Cancelled"]
    );
    tokio::time::timeout(Duration::from_secs(5), observed)
        .await
        .expect("the mock server sees the disconnect")
        .unwrap();
    harness.wait_until_idle().await;
    let events = harness.events(&session.id).await;
    assert_eq!(event_labels(&events), [(1, "user"), (2, "assistant")]);
    let EventBody::Assistant {
        text, tool_calls, ..
    } = &events[1].body
    else {
        panic!("event 2 is not an assistant event");
    };
    assert_eq!(text, "Partial answer");
    assert!(tool_calls.is_empty());
}

#[tokio::test]
async fn a_closed_connection_during_a_tool_call_cancels_and_releases_the_lock() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness
        .mock
        .push_text(&text_call("read", json!({ "path": "a.rs" })));
    let mut browser = Browser::connect(&harness).await;
    browser.send(&run_start(&session.id, "Read a.rs")).await;
    browser
        .read_until(|frame| matches!(frame, ServerFrame::ToolCall { .. }))
        .await;
    assert_eq!(harness.active_run().await, json!(session.id));

    browser.close().await;

    harness.wait_until_idle().await;
    let events = harness.events(&session.id).await;
    assert_eq!(event_labels(&events), [(1, "user"), (2, "assistant")]);
}

#[tokio::test]
async fn unknown_tool_invalid_arguments_and_duplicate_call_get_error_results() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness.mock.push_text(&format!(
        "{}\n{}",
        text_call("shell", json!({ "cmd": "ls" })),
        text_call("read", json!({}))
    ));
    harness
        .mock
        .push_text(&text_call("read", json!({ "path": "a.rs" })));
    harness
        .mock
        .push_text(&text_call("read", json!({ "path": "a.rs" })));
    harness.mock.push_text("Finished.");
    let mut browser = Browser::connect(&harness).await.answer("read", "1|a");

    let frames = browser.run(&session.id, "Go").await;

    assert_eq!(outcome(&frames), RunOutcome::Complete);
    let events = harness.events(&session.id).await;
    assert_eq!(
        tool_results(&events),
        [
            (
                "shell".to_owned(),
                "error: unknown tool: shell".to_owned(),
                true
            ),
            (
                "read".to_owned(),
                "error: invalid arguments for read: missing path".to_owned(),
                true
            ),
            ("read".to_owned(), "1|a".to_owned(), false),
            (
                "read".to_owned(),
                "error: duplicate call. Use the earlier result.".to_owned(),
                true
            ),
        ]
    );
    let tool_call_frames = frames
        .iter()
        .filter(|frame| matches!(frame, ServerFrame::ToolCall { .. }))
        .count();
    assert_eq!(tool_call_frames, 1);
}

#[tokio::test]
async fn three_malformed_turns_in_a_row_fail_the_run() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    for _ in 0..3 {
        harness
            .mock
            .push_text("<tool_call>\nnot json\n</tool_call>");
    }
    let mut browser = Browser::connect(&harness).await;

    let frames = browser.run(&session.id, "Go").await;

    assert_eq!(
        outcome(&frames),
        RunOutcome::Failed {
            message: "the model did not produce a valid tool call".to_owned()
        }
    );
    let events = harness.events(&session.id).await;
    assert_eq!(
        event_labels(&events),
        [
            (1, "user"),
            (2, "assistant"),
            (3, "tool_result"),
            (4, "assistant"),
            (5, "tool_result"),
            (6, "assistant"),
            (7, "tool_result"),
        ]
    );
    let EventBody::Assistant { tool_calls, .. } = &events[1].body else {
        panic!("event 2 is not an assistant event");
    };
    assert_eq!(
        tool_calls,
        &[ToolCall {
            id: "call_2_malformed_0".to_owned(),
            name: "malformed".to_owned(),
            arguments: json!({}),
        }]
    );
    let problem = r#"error: tool call is not valid JSON. Write <tool_call>{"name": ..., "arguments": {...}}</tool_call>"#;
    assert_eq!(
        tool_results(&events)[0],
        ("malformed".to_owned(), problem.to_owned(), true)
    );
    let requests = harness.mock.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        last_message_content(&requests[1]),
        format!("<tool_response>\n{problem}\n</tool_response>")
    );
}

#[tokio::test]
async fn an_unanswered_tool_call_times_out() {
    let harness = Harness::start_with(Settings {
        tool_timeout_seconds: 1,
        ..Settings::default()
    })
    .await;
    let session = harness.create_session("alpha").await;
    harness
        .mock
        .push_text(&text_call("glob", json!({ "pattern": "*.rs" })));
    harness.mock.push_text("No answer from the tool.");
    let mut browser = Browser::connect(&harness).await;

    let frames = browser.run(&session.id, "Find files").await;

    assert_eq!(outcome(&frames), RunOutcome::Complete);
    assert_eq!(
        tool_results(&harness.events(&session.id).await),
        [("glob".to_owned(), "error: tool timed out".to_owned(), true)]
    );
}

#[tokio::test]
async fn a_late_tool_result_is_ignored() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness.mock.push_text("Hello.");
    let mut browser = Browser::connect(&harness).await;
    browser
        .send(&protocol::ClientFrame::ToolResult {
            call_id: "call_9_0".to_owned(),
            output: "stray".to_owned(),
            is_error: false,
        })
        .await;

    let frames = browser.run(&session.id, "Hi").await;

    assert_eq!(
        frames[0],
        ServerFrame::RunStarted {
            session_id: session.id.clone()
        }
    );
    assert_eq!(outcome(&frames), RunOutcome::Complete);
}

#[tokio::test]
async fn the_step_limit_ends_the_run() {
    let harness = Harness::start_with(Settings {
        step_limit: 2,
        ..Settings::default()
    })
    .await;
    let session = harness.create_session("alpha").await;
    harness
        .mock
        .push_text(&text_call("read", json!({ "path": "a.rs" })));
    harness
        .mock
        .push_text(&text_call("read", json!({ "path": "b.rs" })));
    let mut browser = Browser::connect(&harness).await.answer("read", "1|x");

    let frames = browser.run(&session.id, "Read all").await;

    assert_eq!(outcome(&frames), RunOutcome::StepLimit);
    assert_eq!(harness.mock.requests().len(), 2);
}

#[tokio::test]
async fn attachments_are_capped_at_the_tool_output_limit() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness.mock.push_text("Seen.");
    let mut browser = Browser::connect(&harness).await;
    browser
        .send(&protocol::ClientFrame::RunStart {
            session_id: session.id.clone(),
            text: String::new(),
            attachments: vec![protocol::Attachment {
                path: "big.txt".to_owned(),
                lines: None,
                content: "y".repeat(2005),
            }],
            workspace: workspace(&[]),
            capabilities: Vec::new(),
            repo_rules: None,
        })
        .await;
    assert_eq!(
        outcome(&browser.finish(&session.id).await),
        RunOutcome::Complete
    );

    let events = harness.events(&session.id).await;
    let EventBody::User { attachments, .. } = &events[0].body else {
        panic!("event 1 is not a user event");
    };
    assert_eq!(
        attachments[0].content,
        format!(
            "{}\n[output truncated at 2000 characters]",
            "y".repeat(2000)
        )
    );
}

#[tokio::test]
async fn delegate_runs_a_child_session_with_another_model() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness.mock.push_text(&text_call(
        "delegate",
        json!({ "task": "Find the bug", "model": "beta" }),
    ));
    harness
        .mock
        .push_text(&text_call("read", json!({ "path": "b.rs" })));
    harness.mock.push_text("The bug is in b.rs line 3.");
    harness.mock.push_text("Fixed plan ready.");
    let mut browser = Browser::connect(&harness).await.answer("read", "3|bug()");

    let frames = browser.run(&session.id, "Find and explain the bug").await;

    assert_eq!(outcome(&frames), RunOutcome::Complete);
    assert_eq!(
        harness.mock.models_requested(),
        ["alpha", "beta", "beta", "alpha"]
    );
    let (_, sessions) = harness.get_json("/api/sessions").await;
    let sessions: Vec<protocol::Session> = serde_json::from_value(sessions).unwrap();
    let child = sessions
        .iter()
        .find(|candidate| candidate.id != session.id)
        .unwrap();
    assert_eq!(
        child.origin,
        SessionOrigin::Delegate {
            parent: session.id.clone(),
            call_id: "call_2_0".to_owned()
        }
    );
    assert_eq!(child.model, "beta");
    let child_events = harness.events(&child.id).await;
    assert_eq!(
        event_labels(&child_events),
        [
            (1, "user"),
            (2, "assistant"),
            (3, "tool_result"),
            (4, "assistant")
        ]
    );
    let EventBody::User {
        text, workspace, ..
    } = &child_events[0].body
    else {
        panic!("child event 1 is not a user event");
    };
    assert_eq!(text, "Find the bug");
    assert_eq!(
        workspace.as_ref(),
        Some(&crate::support::workspace(&["src/"]))
    );
    assert_eq!(
        tool_results(&harness.events(&session.id).await),
        [(
            "delegate".to_owned(),
            "The bug is in b.rs line 3.".to_owned(),
            false
        )]
    );
    let requests = harness.mock.requests();
    let system = |index: usize| {
        requests[index]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(system(0).contains("delegate("));
    assert!(system(0).contains("beta: Second model"));
    assert!(!system(0).contains("alpha: General model"));
    assert!(!system(1).contains("delegate"));
    let child_frame_count = frames
        .iter()
        .filter(|frame| matches!(frame, ServerFrame::Event { session_id, .. } if *session_id == child.id))
        .count();
    assert_eq!(child_frame_count, 4);
}

#[tokio::test]
async fn delegate_with_an_unknown_model_gets_the_choice_list() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness.mock.push_text(&text_call(
        "delegate",
        json!({ "task": "Check", "model": "gamma" }),
    ));
    harness.mock.push_text("I will do it myself.");
    let mut browser = Browser::connect(&harness).await;

    let frames = browser.run(&session.id, "Check it").await;

    assert_eq!(outcome(&frames), RunOutcome::Complete);
    assert_eq!(
        tool_results(&harness.events(&session.id).await),
        [(
            "delegate".to_owned(),
            "error: unknown model: gamma. Choose one of: beta".to_owned(),
            true
        )]
    );
    assert_eq!(harness.mock.models_requested(), ["alpha", "alpha"]);
}

#[tokio::test]
async fn a_model_switch_between_runs_keeps_the_history() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    let mut browser = Browser::connect(&harness).await;
    harness.mock.push_text("Answer one.");
    browser.run(&session.id, "First question").await;
    harness.wait_until_idle().await;

    let response = harness.patch(&session.id, json!({ "model": "beta" })).await;
    assert_eq!(response.status(), 200);
    harness.mock.push_text("Answer two.");
    let frames = browser.run(&session.id, "Second question").await;

    assert_eq!(outcome(&frames), RunOutcome::Complete);
    let requests = harness.mock.requests();
    assert_eq!(requests[1]["model"], "beta");
    let history = all_content(&requests[1]);
    assert!(history.contains("First question"));
    assert!(history.contains("Answer one."));
    let events = harness.events(&session.id).await;
    assert_eq!(
        event_labels(&events),
        [
            (1, "user"),
            (2, "assistant"),
            (3, "model_switch"),
            (4, "user"),
            (5, "assistant")
        ]
    );
    assert_eq!(
        events[2].body,
        EventBody::ModelSwitch {
            from: "alpha".to_owned(),
            to: "beta".to_owned()
        }
    );
}

#[tokio::test]
async fn a_run_on_a_fork_leaves_the_parent_unchanged() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    let mut browser = Browser::connect(&harness).await;
    harness.mock.push_text("One.");
    browser.run(&session.id, "FIRST-QUESTION").await;
    harness.mock.push_text("Two.");
    browser.run(&session.id, "SECOND-QUESTION").await;
    harness.wait_until_idle().await;

    let fork: protocol::Session = harness
        .http
        .post(harness.url(&format!("/api/sessions/{}/fork", session.id)))
        .header("origin", ORIGIN)
        .json(&json!({ "seq": 2 }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    harness.mock.push_text("Forked answer.");
    let frames = browser.run(&fork.id, "Other second").await;

    assert_eq!(outcome(&frames), RunOutcome::Complete);
    let fork_history = all_content(&harness.mock.requests()[2]);
    assert!(fork_history.contains("FIRST-QUESTION"));
    assert!(!fork_history.contains("SECOND-QUESTION"));
    let parent_events = harness.events(&session.id).await;
    assert_eq!(
        event_labels(&parent_events),
        [(1, "user"), (2, "assistant"), (3, "user"), (4, "assistant")]
    );
    let EventBody::User { text, .. } = &parent_events[2].body else {
        panic!("parent event 3 is not a user event");
    };
    assert_eq!(text, "SECOND-QUESTION");
    assert_eq!(
        event_labels(&harness.events(&fork.id).await),
        [(1, "user"), (2, "assistant"), (3, "user"), (4, "assistant")]
    );
}

#[tokio::test]
async fn a_long_session_is_elided_and_then_summarized() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    let mut browser = Browser::connect(&harness).await;
    for path in ["a.rs", "b.rs", "c.rs", "d.rs"] {
        harness
            .mock
            .push_text(&text_call("read", json!({ "path": path })));
    }
    harness.mock.push_text("Read all four.");
    let large = "L".repeat(2000);
    let mut sequence = vec![large.clone(), large, "1|c".to_owned(), "1|d".to_owned()].into_iter();
    browser
        .send(&run_start(&session.id, "OLDEST QUESTION"))
        .await;
    let mut frames = Vec::new();
    loop {
        let frame = browser.next_frame().await;
        if let ServerFrame::ToolCall { call, .. } = &frame {
            browser
                .send(&protocol::ClientFrame::ToolResult {
                    call_id: call.id.clone(),
                    output: sequence.next().unwrap(),
                    is_error: false,
                })
                .await;
        }
        let finished = matches!(frame, ServerFrame::RunFinished { .. });
        frames.push(frame);
        if finished {
            break;
        }
    }
    assert_eq!(outcome(&frames), RunOutcome::Complete);
    harness.wait_until_idle().await;
    assert_eq!(
        harness
            .patch(&session.id, json!({ "model": "tiny" }))
            .await
            .status(),
        200
    );

    harness.mock.push_text("Short answer.");
    let frames = browser.run(&session.id, "Next question").await;
    assert_eq!(outcome(&frames), RunOutcome::Complete);
    let events = harness.events(&session.id).await;
    assert_eq!(
        events[12].body,
        EventBody::Compaction {
            through: 5,
            kind: CompactionKind::Elide
        }
    );

    harness.mock.push_text("SUMMARY: the user read four files.");
    harness.mock.push_text("Final answer.");
    let question = "Q".repeat(3000);
    let frames = browser.run(&session.id, &question).await;
    assert_eq!(outcome(&frames), RunOutcome::Complete);
    let events = harness.events(&session.id).await;
    let summaries: Vec<(u32, u32, String)> = events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::Compaction {
                through,
                kind: CompactionKind::Summary { text, model },
            } => Some((event.seq, *through, format!("{model}: {text}"))),
            _ => None,
        })
        .collect();
    assert_eq!(
        summaries,
        [(16, 7, "tiny: SUMMARY: the user read four files.".to_owned())]
    );
    let requests = harness.mock.requests();
    let summary_request = &requests[requests.len() - 2];
    assert!(summary_request.get("tools").is_none());
    assert_eq!(summary_request["max_tokens"], 200);
    let after = all_content(requests.last().unwrap());
    assert!(after.contains("SUMMARY: the user read four files."));
    assert!(!after.contains("OLDEST QUESTION"));
}

#[tokio::test]
async fn the_workspace_snapshot_is_stored_on_first_use_and_on_change() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    let mut browser = Browser::connect(&harness).await;
    let snapshots = [
        workspace(&["src/"]),
        workspace(&["src/"]),
        workspace(&["src/", "docs/"]),
    ];
    for snapshot in &snapshots {
        harness.mock.push_text("Ok.");
        browser
            .send(&run_start_in(&session.id, "Next", snapshot.clone()))
            .await;
        browser.finish(&session.id).await;
    }

    let stored: Vec<Option<protocol::WorkspaceSnapshot>> = harness
        .events(&session.id)
        .await
        .into_iter()
        .filter_map(|event| match event.body {
            EventBody::User { workspace, .. } => Some(workspace),
            _ => None,
        })
        .collect();
    assert_eq!(
        stored,
        [
            Some(workspace(&["src/"])),
            None,
            Some(workspace(&["src/", "docs/"]))
        ]
    );
}

fn user_rules(events: &[Event]) -> Vec<(Option<String>, Option<String>)> {
    events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::User {
                system_rules,
                repo_rules,
                ..
            } => Some((system_rules.clone(), repo_rules.clone())),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn rules_freeze_on_the_first_user_event_and_pass_to_a_delegate() {
    let harness = Harness::start_with(Settings {
        system_rules: Some("Prefer small diffs."),
        ..Settings::default()
    })
    .await;
    let session = harness.create_session("alpha").await;
    harness
        .mock
        .push_text(&text_call("delegate", json!({ "task": "Sub task" })));
    harness.mock.push_text("Sub result.");
    harness.mock.push_text("Done.");
    let mut browser = Browser::connect(&harness).await;
    browser
        .send(&run_start_with_rules(
            &session.id,
            "Main task",
            Some("Use pnpm."),
        ))
        .await;
    assert_eq!(
        outcome(&browser.finish(&session.id).await),
        RunOutcome::Complete
    );

    harness.mock.push_text("Next.");
    browser
        .send(&run_start_with_rules(
            &session.id,
            "Again",
            Some("Use npm."),
        ))
        .await;
    assert_eq!(
        outcome(&browser.finish(&session.id).await),
        RunOutcome::Complete
    );

    let fresh = harness.create_session("alpha").await;
    harness.mock.push_text("Ok.");
    browser
        .send(&run_start_with_rules(&fresh.id, "Plain", Some("   ")))
        .await;
    assert_eq!(
        outcome(&browser.finish(&fresh.id).await),
        RunOutcome::Complete
    );

    assert_eq!(
        user_rules(&harness.events(&session.id).await),
        [
            (
                Some("Prefer small diffs.".to_owned()),
                Some("Use pnpm.".to_owned())
            ),
            (None, None),
        ]
    );
    assert_eq!(
        user_rules(&harness.events(&fresh.id).await),
        [(Some("Prefer small diffs.".to_owned()), None)]
    );
    let (_, sessions) = harness.get_json("/api/sessions").await;
    let child_id = sessions
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| candidate["id"].as_str().unwrap().to_owned())
        .find(|id| *id != session.id && *id != fresh.id)
        .unwrap();
    assert_eq!(
        user_rules(&harness.events(&child_id).await),
        [(
            Some("Prefer small diffs.".to_owned()),
            Some("Use pnpm.".to_owned())
        )]
    );

    let requests = harness.mock.requests();
    assert_eq!(requests.len(), 5);
    let system_message = |index: usize| {
        requests[index]["messages"][0]["content"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let first = system_message(0);
    let tools_at = first.find("\n\nTools:").unwrap();
    let system_at = first
        .find("\n\nSystem rules:\nPrefer small diffs.")
        .unwrap();
    let repo_at = first.find("\n\nRepo rules:\nUse pnpm.").unwrap();
    assert!(tools_at < system_at && system_at < repo_at);
    assert!(system_message(1).contains("\n\nRepo rules:\nUse pnpm."));
    assert!(system_message(3).contains("\n\nRepo rules:\nUse pnpm."));
    assert!(system_message(4).contains("\n\nSystem rules:\nPrefer small diffs."));
    assert!(!system_message(4).contains("Repo rules:"));
    assert!(
        requests
            .iter()
            .all(|request| !all_content(request).contains("Use npm."))
    );
}

#[tokio::test]
async fn invalid_run_starts_are_rejected() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    let mut browser = Browser::connect(&harness).await;
    let invalid = |message: &str| ServerFrame::Rejected {
        reason: RejectReason::InvalidRequest {
            message: message.to_owned(),
        },
    };

    browser.send(&run_start("unknown", "Hi")).await;
    assert_eq!(browser.next_frame().await, invalid("session not found"));
    browser.send(&run_start(&session.id, "  ")).await;
    assert_eq!(
        browser.next_frame().await,
        invalid("the run needs a text or an attachment")
    );
    browser.send_text("not json").await;
    assert!(matches!(
        browser.next_frame().await,
        ServerFrame::Rejected {
            reason: RejectReason::InvalidRequest { .. }
        }
    ));
    let too_long = "x".repeat(RULES_CHARACTER_LIMIT + 1);
    browser
        .send(&run_start_with_rules(&session.id, "Hi", Some(&too_long)))
        .await;
    assert_eq!(
        browser.next_frame().await,
        invalid(&format!(
            "repo rules must hold at most {RULES_CHARACTER_LIMIT} characters"
        ))
    );
    assert!(harness.active_run().await.is_null());
}

#[tokio::test]
async fn a_delegate_child_session_takes_no_run() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness
        .mock
        .push_text(&text_call("delegate", json!({ "task": "Sub task" })));
    harness.mock.push_text("Sub result.");
    harness.mock.push_text("Done.");
    let mut browser = Browser::connect(&harness).await;
    browser.run(&session.id, "Main task").await;
    harness.wait_until_idle().await;
    assert_eq!(harness.mock.models_requested(), ["alpha", "alpha", "alpha"]);
    let (_, sessions) = harness.get_json("/api/sessions").await;
    let child_id = sessions
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| candidate["id"].as_str().unwrap().to_owned())
        .find(|id| *id != session.id)
        .unwrap();

    browser.send(&run_start(&child_id, "Again")).await;

    assert_eq!(
        browser.next_frame().await,
        ServerFrame::Rejected {
            reason: RejectReason::InvalidRequest {
                message: "a delegate child session takes no run".to_owned()
            }
        }
    );
}

#[tokio::test]
async fn a_reply_past_the_size_limit_fails_the_run_and_drops_the_request() {
    let harness = Harness::start().await;
    let session = harness.create_session("capped").await;
    let (dropped, observed) = oneshot::channel();
    harness.mock.push(Script::Hold(
        vec![
            content_chunk(&"x".repeat(40)),
            content_chunk(&"y".repeat(25)),
        ],
        dropped,
    ));
    let mut browser = Browser::connect(&harness).await;

    let frames = browser.run(&session.id, "Start").await;

    assert_eq!(
        outcome(&frames),
        RunOutcome::Failed {
            message: "the model reply passed the size limit".to_owned()
        }
    );
    tokio::time::timeout(Duration::from_secs(5), observed)
        .await
        .expect("the mock server sees the disconnect")
        .unwrap();
    harness.wait_until_idle().await;
    harness.mock.push_text(&"z".repeat(64));
    let frames = browser.run(&session.id, "Again").await;
    assert_eq!(outcome(&frames), RunOutcome::Complete);
}

#[tokio::test]
async fn a_summary_past_the_size_limit_fails_the_run_and_drops_the_request() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    let mut browser = Browser::connect(&harness).await;
    for turn in ["one", "two", "three", "four"] {
        harness.mock.push_text("Noted.");
        let frames = browser.run(&session.id, turn).await;
        assert_eq!(outcome(&frames), RunOutcome::Complete);
    }
    harness.wait_until_idle().await;
    assert_eq!(
        harness
            .patch(&session.id, json!({ "model": "capped" }))
            .await
            .status(),
        200
    );
    let (dropped, observed) = oneshot::channel();
    harness
        .mock
        .push(Script::Hold(vec![content_chunk(&"S".repeat(65))], dropped));
    let question = "Q".repeat(6000);

    let frames = browser.run(&session.id, &question).await;

    assert_eq!(
        outcome(&frames),
        RunOutcome::Failed {
            message: "the model reply passed the size limit".to_owned()
        }
    );
    tokio::time::timeout(Duration::from_secs(5), observed)
        .await
        .expect("the mock server sees the disconnect")
        .unwrap();
    harness.wait_until_idle().await;
    let summary = "S".repeat(64);
    harness.mock.push_text(&summary);
    harness.mock.push_text("Done.");
    let frames = browser.run(&session.id, &question).await;
    assert_eq!(outcome(&frames), RunOutcome::Complete);
    let summaries: Vec<String> = harness
        .events(&session.id)
        .await
        .into_iter()
        .filter_map(|event| match event.body {
            EventBody::Compaction {
                kind: CompactionKind::Summary { text, .. },
                ..
            } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(summaries, [summary]);
}
