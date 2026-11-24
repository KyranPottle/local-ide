use protocol::{ClientFrame, RunOutcome, ServerFrame};
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::support::*;

async fn post(harness: &Harness, path: &str, body: &str) -> (u16, Value) {
    let response = harness
        .http
        .post(harness.url(path))
        .header("origin", ORIGIN)
        .header("content-type", "application/json")
        .body(body.to_owned())
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn health_and_status() {
    let harness = Harness::start().await;
    let response = harness
        .http
        .get(harness.url("/api/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.unwrap(), "ok");
    let expected_version = workspace_release_version();
    assert_eq!(
        harness.get_json("/api/status").await,
        (
            200,
            json!({ "version": expected_version, "active_run": null })
        )
    );
}

#[tokio::test]
async fn models_list_the_profiles_and_the_unprofiled_server_models() {
    let harness = Harness::start().await;
    let (status, models) = harness.get_json("/api/models").await;
    assert_eq!(status, 200);
    let summary: Vec<(String, String, bool)> = models
        .as_array()
        .unwrap()
        .iter()
        .map(|model| {
            (
                model["profile"]["id"].as_str().unwrap().to_owned(),
                model["profile"]["label"].as_str().unwrap().to_owned(),
                model["available"].as_bool().unwrap(),
            )
        })
        .collect();
    let expected = [
        ("alpha", "Alpha", true),
        ("beta", "Beta", true),
        ("native", "Native", true),
        ("tiny", "Tiny", true),
        ("gamma", "Gamma", false),
        ("capped", "Capped", true),
        ("extra", "extra", true),
    ]
    .map(|(id, label, available)| (id.to_owned(), label.to_owned(), available));
    assert_eq!(summary, expected);
    assert_eq!(models[6]["profile"]["context_budget"], 16384);
}

#[tokio::test]
async fn models_without_an_inference_server_are_unavailable() {
    let harness = Harness::start().await;
    harness.mock.fail_model_list();
    let (status, models) = harness.get_json("/api/models").await;
    assert_eq!(status, 200);
    let available: Vec<bool> = models
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["available"].as_bool().unwrap())
        .collect();
    assert_eq!(available, [false, false, false, false, false, false]);
}

#[tokio::test]
async fn session_routes_and_their_error_cases() {
    let harness = Harness::start().await;
    let (status, created) = post(
        &harness,
        "/api/sessions",
        r#"{"workspace":"demo","model":"alpha","mode":"ask"}"#,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(created["title"], "");
    assert_eq!(created["mode"], "ask");
    assert_eq!(created["origin"], json!({ "kind": "root" }));
    let id = created["id"].as_str().unwrap().to_owned();

    let long_model = format!(
        r#"{{"workspace":"demo","model":"{}","mode":"agent"}}"#,
        "m".repeat(201)
    );
    for body in [
        r#"{"workspace":"","model":"alpha","mode":"agent"}"#,
        r#"{"workspace":"demo","model":" ","mode":"agent"}"#,
        long_model.as_str(),
        r#"{"workspace":"demo","model":"alpha","mode":"shell"}"#,
        "not json",
    ] {
        let (status, error) = post(&harness, "/api/sessions", body).await;
        assert_eq!(status, 400, "{body}");
        assert!(error["message"].is_string());
    }

    let (status, detail) = harness.get_json(&format!("/api/sessions/{id}")).await;
    assert_eq!(status, 200);
    assert_eq!(detail["events"], json!([]));
    assert_eq!(detail["context"]["budget"], 16000);
    assert_eq!(detail["context"]["reserve"], 1000);
    assert_eq!(detail["context"]["history"], 0);
    assert_eq!(
        harness.get_json("/api/sessions/missing").await,
        (404, json!({ "message": "session not found" }))
    );

    let response = harness
        .patch(&id, json!({ "title": "Renamed", "mode": "agent" }))
        .await;
    assert_eq!(response.status(), 200);
    let updated: Value = response.json().await.unwrap();
    assert_eq!(
        (updated["title"].clone(), updated["mode"].clone()),
        (json!("Renamed"), json!("agent"))
    );
    assert_eq!(
        harness.patch(&id, json!({ "model": "" })).await.status(),
        400
    );
    assert_eq!(
        harness
            .patch("missing", json!({ "title": "x" }))
            .await
            .status(),
        404
    );

    let (status, _) = post(
        &harness,
        &format!("/api/sessions/{id}/fork"),
        r#"{"seq":1}"#,
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = post(&harness, "/api/sessions/missing/fork", r#"{"seq":1}"#).await;
    assert_eq!(status, 404);

    let (status, unknown) = harness.get_json("/api/unknown").await;
    assert_eq!((status, unknown), (404, json!({ "message": "not found" })));

    assert_eq!(harness.delete(&id).await.status(), 204);
    assert_eq!(
        harness.get_json(&format!("/api/sessions/{id}")).await.0,
        404
    );
    assert_eq!(harness.delete(&id).await.status(), 404);
}

#[tokio::test]
async fn session_list_is_newest_first_and_searchable() {
    let harness = Harness::start().await;
    let first = harness.create_session("alpha").await;
    let second = harness.create_session("alpha").await;
    let mut browser = Browser::connect(&harness).await;
    harness.mock.push_text("About parsers.");
    browser.run(&first.id, "Explain the tokenizer").await;

    let (_, all) = harness.get_json("/api/sessions").await;
    let ids: Vec<&str> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [first.id.as_str(), second.id.as_str()]);
    let (_, found) = harness.get_json("/api/sessions?q=tokenizer").await;
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["id"], json!(first.id));
    assert_eq!(found[0]["title"], "Explain the tokenizer");
    let (_, none) = harness.get_json("/api/sessions?q=compiler").await;
    assert_eq!(none, json!([]));
    let long_query = format!("/api/sessions?q={}", "q".repeat(201));
    assert_eq!(harness.get_json(&long_query).await.0, 400);
}

#[tokio::test]
async fn patch_and_delete_conflict_with_the_active_run_and_fork_does_not() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    let mut browser = Browser::connect(&harness).await;
    harness.mock.push_text("Prior.");
    browser.run(&session.id, "Prior").await;
    harness.wait_until_idle().await;
    let (dropped, _observed) = oneshot::channel();
    harness
        .mock
        .push(Script::Hold(vec![content_chunk("Thinking")], dropped));
    browser.send(&run_start(&session.id, "Long")).await;
    browser
        .read_until(|frame| matches!(frame, ServerFrame::Delta { .. }))
        .await;

    assert_eq!(harness.active_run().await, json!(session.id));
    let conflict = harness.patch(&session.id, json!({ "title": "x" })).await;
    assert_eq!(conflict.status(), 409);
    assert_eq!(
        conflict.json::<Value>().await.unwrap(),
        json!({ "message": "the session has the active run" })
    );
    assert_eq!(harness.delete(&session.id).await.status(), 409);
    let (status, _) = post(
        &harness,
        &format!("/api/sessions/{}/fork", session.id),
        r#"{"seq":1}"#,
    )
    .await;
    assert_eq!(status, 200);

    browser.send(&ClientFrame::RunCancel).await;
    assert_eq!(
        outcome(&browser.finish(&session.id).await),
        RunOutcome::Cancelled
    );
    harness.wait_until_idle().await;
    assert_eq!(
        harness
            .patch(&session.id, json!({ "title": "x" }))
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn an_active_delegate_child_blocks_its_parent_and_itself() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness
        .mock
        .push_text(&text_call("delegate", json!({ "task": "Sub task" })));
    let (dropped, _observed) = oneshot::channel();
    harness
        .mock
        .push(Script::Hold(vec![content_chunk("Child works")], dropped));
    let mut browser = Browser::connect(&harness).await;
    browser.send(&run_start(&session.id, "Main")).await;
    let frames = browser
        .read_until(|frame| matches!(frame, ServerFrame::Delta { .. }))
        .await;
    let child_id = frames
        .iter()
        .find_map(|frame| match frame {
            ServerFrame::SessionChanged { session: child } if child.id != session.id => {
                Some(child.id.clone())
            }
            _ => None,
        })
        .unwrap();

    assert_eq!(harness.active_run().await, json!(session.id));
    assert_eq!(
        harness
            .patch(&session.id, json!({ "title": "x" }))
            .await
            .status(),
        409
    );
    assert_eq!(
        harness
            .patch(&child_id, json!({ "title": "x" }))
            .await
            .status(),
        409
    );
    assert_eq!(harness.delete(&child_id).await.status(), 409);

    browser.send(&ClientFrame::RunCancel).await;
    let frames = browser.finish(&session.id).await;
    assert_eq!(outcome(&frames), RunOutcome::Cancelled);
    assert!(frames.iter().any(|frame| matches!(
        frame,
        ServerFrame::RunFinished { session_id, outcome: RunOutcome::Cancelled } if *session_id == child_id
    )));
}

#[tokio::test]
async fn delete_removes_the_delegate_children() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness
        .mock
        .push_text(&text_call("delegate", json!({ "task": "Sub task" })));
    harness.mock.push_text("Sub result.");
    harness.mock.push_text("Done.");
    let mut browser = Browser::connect(&harness).await;
    browser.run(&session.id, "Main").await;
    harness.wait_until_idle().await;
    let (_, sessions) = harness.get_json("/api/sessions").await;
    assert_eq!(sessions.as_array().unwrap().len(), 2);

    assert_eq!(harness.delete(&session.id).await.status(), 204);

    assert_eq!(harness.get_json("/api/sessions").await.1, json!([]));
}

#[tokio::test]
async fn static_files_fall_back_to_the_index() {
    let harness = Harness::start().await;
    let page = harness
        .http
        .get(harness.url("/editor/route"))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 200);
    assert_eq!(page.headers()["cache-control"], "no-cache");
    assert_eq!(
        page.text().await.unwrap(),
        "<!doctype html><title>local-ide</title>"
    );

    let script = harness
        .http
        .get(harness.url("/assets/app.js"))
        .send()
        .await
        .unwrap();
    assert_eq!(script.status(), 200);
    assert_eq!(
        script.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );
    assert_eq!(script.text().await.unwrap(), "console.log(1)");

    let compressed = harness
        .http
        .get(harness.url("/assets/app.js"))
        .header("accept-encoding", "gzip")
        .send()
        .await
        .unwrap();
    assert_eq!(compressed.headers()["content-encoding"], "gzip");
    assert_eq!(compressed.bytes().await.unwrap().as_ref(), b"gzip bytes");

    let missing = harness
        .http
        .get(harness.url("/assets/missing.js"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
}
