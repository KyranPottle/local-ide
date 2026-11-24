use axum::http::StatusCode;
use protocol::{RunOutcome, ServerFrame};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::support::*;

#[tokio::test]
async fn a_peer_outside_allowed_peers_is_rejected() {
    let harness = Harness::start_with(Settings {
        allowed_peer: "10.9.9.9",
        ..Settings::default()
    })
    .await;
    for path in ["/api/health", "/", "/api/sessions"] {
        let response = harness.http.get(harness.url(path)).send().await.unwrap();
        assert_eq!(response.status(), 403, "{path}");
    }
    let socket = connect_with_origin(&harness, Some(ORIGIN)).await;
    assert!(socket.is_err());
}

#[tokio::test]
async fn a_mutation_without_the_public_origin_is_rejected() {
    let harness = Harness::start().await;
    let body = json!({ "workspace": "demo", "model": "alpha", "mode": "agent" });
    let missing = harness
        .http
        .post(harness.url("/api/sessions"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 403);
    let wrong = harness
        .http
        .post(harness.url("/api/sessions"))
        .header("origin", "http://evil.test")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 403);
    let session = harness.create_session("alpha").await;
    let delete = harness
        .http
        .delete(harness.url(&format!("/api/sessions/{}", session.id)))
        .header("origin", "http://evil.test")
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), 403);
    assert_eq!(
        harness
            .get_json("/api/sessions")
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_websocket_upgrade_without_the_public_origin_is_rejected() {
    let harness = Harness::start().await;
    for origin in [None, Some("http://evil.test")] {
        let error = connect_with_origin(&harness, origin).await.unwrap_err();
        let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
            panic!("the upgrade fails with an HTTP status");
        };
        assert_eq!(response.status(), 403);
    }
}

#[tokio::test]
async fn the_api_key_reaches_the_server_and_no_response() {
    let harness = Harness::start().await;
    let session = harness.create_session("alpha").await;
    harness.mock.push_text("Hello.");
    harness.mock.push(Script::Status(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("upstream echo: Bearer {API_KEY}"),
    ));
    let mut browser = Browser::connect(&harness).await;
    let mut frames = browser.run(&session.id, "Hi").await;
    frames.extend(browser.run(&session.id, "Again").await);

    assert_eq!(
        outcome(&frames),
        RunOutcome::Failed {
            message: "inference server returned HTTP 500: upstream echo: Bearer [redacted]"
                .to_owned()
        }
    );
    let frame_text = serde_json::to_string(&frames).unwrap();
    assert!(!frame_text.contains(API_KEY));
    let mut bodies = vec![frame_text];
    for path in [
        "/api/health",
        "/api/status",
        "/api/models",
        "/api/sessions",
        &format!("/api/sessions/{}", session.id),
        "/",
    ] {
        let response = harness.http.get(harness.url(path)).send().await.unwrap();
        bodies.push(response.text().await.unwrap());
    }
    for body in bodies {
        assert!(!body.contains(API_KEY));
    }
    let expected = Some(format!("Bearer {API_KEY}"));
    assert_eq!(harness.mock.authorizations(), [expected.clone(), expected]);
    assert!(
        frames
            .iter()
            .any(|frame| matches!(frame, ServerFrame::RunFinished { .. }))
    );
}

#[tokio::test]
async fn static_responses_carry_the_security_headers() {
    let harness = Harness::start().await;
    for path in ["/", "/api/health"] {
        let response = harness.http.get(harness.url(path)).send().await.unwrap();
        let headers = response.headers();
        assert_eq!(headers["x-content-type-options"], "nosniff", "{path}");
        assert_eq!(headers["referrer-policy"], "no-referrer", "{path}");
        assert_eq!(
            headers["content-security-policy"],
            "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'",
            "{path}"
        );
    }
}

async fn raw_get(harness: &Harness, target: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(harness.address)
        .await
        .unwrap();
    let request = format!("GET {target} HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn a_static_path_with_parent_segments_stays_in_the_assets_directory() {
    let harness = Harness::start().await;
    for target in [
        "/../secret.txt",
        "/assets/../../secret.txt",
        "/%2e%2e/secret.txt",
        "/assets/%2e%2e/%2e%2e/secret.txt",
        "/..%2fsecret.txt",
    ] {
        let response = raw_get(&harness, target).await;
        assert!(!response.contains("outside the assets"), "{target}");
    }
}
