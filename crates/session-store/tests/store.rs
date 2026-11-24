use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use protocol::{EventBody, Mode, SessionOrigin};
use session_store::{NewSession, SessionStore, SessionUpdate, StoreError};

struct TestClock(Arc<AtomicI64>);

impl TestClock {
    fn set(&self, millis: i64) {
        self.0.store(millis, Ordering::SeqCst);
    }
}

fn store() -> (SessionStore, TestClock) {
    let time = Arc::new(AtomicI64::new(1_000));
    let source = Arc::clone(&time);
    let store = SessionStore::open_in_memory(move || source.load(Ordering::SeqCst)).unwrap();
    (store, TestClock(time))
}

fn new_session(origin: SessionOrigin) -> NewSession {
    NewSession {
        workspace: "demo".to_owned(),
        model: "coder".to_owned(),
        mode: Mode::Agent,
        origin,
    }
}

fn user(text: &str) -> EventBody {
    EventBody::User {
        text: text.to_owned(),
        attachments: Vec::new(),
        workspace: None,
        system_rules: None,
        repo_rules: None,
    }
}

fn assistant(text: &str) -> EventBody {
    EventBody::Assistant {
        text: text.to_owned(),
        reasoning: "hidden thought".to_owned(),
        tool_calls: Vec::new(),
        model: "coder".to_owned(),
        usage: None,
    }
}

fn tool_result(output: &str) -> EventBody {
    EventBody::ToolResult {
        call_id: "c1".to_owned(),
        name: "read".to_owned(),
        output: output.to_owned(),
        is_error: false,
    }
}

fn ids(sessions: &[protocol::Session]) -> Vec<String> {
    sessions.iter().map(|session| session.id.clone()).collect()
}

#[test]
fn bundled_sqlite_has_fts5() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    let enabled: bool = connection
        .query_row(
            "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(enabled);
}

#[test]
fn creates_and_reads_each_origin() {
    let (mut store, _) = store();
    let root = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    let fork_origin = SessionOrigin::Fork {
        parent: root.id.clone(),
        seq: 3,
    };
    let delegate_origin = SessionOrigin::Delegate {
        parent: root.id.clone(),
        call_id: "call-7".to_owned(),
    };
    let fork = store
        .create_session(new_session(fork_origin.clone()))
        .unwrap();
    let delegate = store
        .create_session(new_session(delegate_origin.clone()))
        .unwrap();

    assert_eq!(root.title, "");
    assert_eq!(root.created_at, 1_000);
    assert_eq!(
        uuid::Uuid::parse_str(&root.id).unwrap().get_version_num(),
        7
    );
    assert_eq!(store.session(&root.id).unwrap(), root);
    assert_eq!(store.session(&fork.id).unwrap().origin, fork_origin);
    assert_eq!(store.session(&delegate.id).unwrap().origin, delegate_origin);
    assert_eq!(store.session(&delegate.id).unwrap(), delegate);
}

#[test]
fn numbers_events_and_uses_clock() {
    let (mut store, clock) = store();
    let session = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    clock.set(2_000);
    let first = store.append_event(&session.id, user("hello")).unwrap();
    clock.set(3_500);
    let second = store.append_event(&session.id, assistant("hi")).unwrap();

    assert_eq!((first.seq, first.at), (1, 2_000));
    assert_eq!((second.seq, second.at), (2, 3_500));
    assert_eq!(store.events(&session.id).unwrap(), vec![first, second]);
    assert_eq!(store.session(&session.id).unwrap().updated_at, 3_500);
}

#[test]
fn default_title_from_first_user_text() {
    let (mut store, _) = store();
    let multi_byte = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    let text = "  ".to_owned() + &"é".repeat(70);
    store.append_event(&multi_byte.id, user(&text)).unwrap();
    assert_eq!(store.session(&multi_byte.id).unwrap().title, "é".repeat(60));

    let multi_line = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    store
        .append_event(&multi_line.id, user("  Fix the parser  \nsecond line"))
        .unwrap();
    store
        .append_event(&multi_line.id, user("later message"))
        .unwrap();
    assert_eq!(
        store.session(&multi_line.id).unwrap().title,
        "Fix the parser"
    );
}

#[test]
fn list_orders_by_last_update() {
    let (mut store, clock) = store();
    let older = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    clock.set(2_000);
    let newer = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    assert_eq!(
        ids(&store.list_sessions(None).unwrap()),
        vec![newer.id.clone(), older.id.clone()]
    );

    clock.set(3_000);
    store.append_event(&older.id, user("bump")).unwrap();
    assert_eq!(
        ids(&store.list_sessions(None).unwrap()),
        vec![older.id.clone(), newer.id.clone()]
    );
    assert_eq!(
        ids(&store.list_sessions(Some("   ")).unwrap()),
        vec![older.id, newer.id]
    );
}

#[test]
fn model_change_appends_one_switch_event() {
    let (mut store, _) = store();
    let session = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    let unchanged = SessionUpdate {
        model: Some("coder".to_owned()),
        ..SessionUpdate::default()
    };
    store.update_session(&session.id, unchanged).unwrap();
    assert!(store.events(&session.id).unwrap().is_empty());

    let update = SessionUpdate {
        title: Some("Renamed".to_owned()),
        model: Some("llama".to_owned()),
        mode: Some(Mode::Ask),
    };
    let updated = store.update_session(&session.id, update).unwrap();
    assert_eq!(
        (updated.title.as_str(), updated.model.as_str(), updated.mode),
        ("Renamed", "llama", Mode::Ask)
    );
    assert_eq!(store.session(&session.id).unwrap(), updated);
    let events = store.events(&session.id).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].body,
        EventBody::ModelSwitch {
            from: "coder".to_owned(),
            to: "llama".to_owned()
        }
    );
}

#[test]
fn fork_copies_prefix_and_stays_independent() {
    let (mut store, clock) = store();
    let parent = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    for (at, text) in [(2_000, "one"), (3_000, "two"), (4_000, "three")] {
        clock.set(at);
        store.append_event(&parent.id, user(text)).unwrap();
    }
    clock.set(5_000);
    let fork = store.fork_session(&parent.id, 2).unwrap();
    assert_eq!(
        fork.origin,
        SessionOrigin::Fork {
            parent: parent.id.clone(),
            seq: 2
        }
    );
    assert_eq!(fork.title, "one");
    assert_eq!(fork.created_at, 5_000);
    let copied = store.events(&fork.id).unwrap();
    assert_eq!(
        copied
            .iter()
            .map(|event| (event.seq, event.at))
            .collect::<Vec<_>>(),
        vec![(1, 2_000), (2, 3_000)]
    );

    store.append_event(&fork.id, user("fork only")).unwrap();
    store.append_event(&parent.id, user("parent only")).unwrap();
    assert_eq!(store.events(&fork.id).unwrap()[2].body, user("fork only"));
    assert_eq!(
        store.events(&parent.id).unwrap()[3].body,
        user("parent only")
    );
    assert_eq!(store.events(&fork.id).unwrap().len(), 3);
    assert_eq!(store.events(&parent.id).unwrap().len(), 4);
    assert_eq!(ids(&store.list_sessions(Some("two")).unwrap()).len(), 2);
}

#[test]
fn fork_rejects_invalid_sequence() {
    let (mut store, _) = store();
    let parent = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    store.append_event(&parent.id, user("one")).unwrap();
    for seq in [0, 2] {
        assert!(
            matches!(store.fork_session(&parent.id, seq), Err(StoreError::InvalidSequence { seq: s, .. }) if s == seq)
        );
    }
}

#[test]
fn delete_removes_delegates_and_keeps_forks() {
    let (mut store, _) = store();
    let root = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    store.append_event(&root.id, user("root text")).unwrap();
    let child = store
        .create_session(new_session(SessionOrigin::Delegate {
            parent: root.id.clone(),
            call_id: "a".to_owned(),
        }))
        .unwrap();
    let grandchild = store
        .create_session(new_session(SessionOrigin::Delegate {
            parent: child.id.clone(),
            call_id: "b".to_owned(),
        }))
        .unwrap();
    let fork = store.fork_session(&root.id, 1).unwrap();

    store.delete_session(&root.id).unwrap();
    for id in [&root.id, &child.id, &grandchild.id] {
        assert!(matches!(
            store.session(id),
            Err(StoreError::SessionNotFound(_))
        ));
    }
    assert_eq!(store.session(&fork.id).unwrap(), fork);
    assert_eq!(store.events(&fork.id).unwrap().len(), 1);
    assert_eq!(
        ids(&store.list_sessions(Some("root")).unwrap()),
        vec![fork.id]
    );
}

#[test]
fn search_covers_titles_and_message_text_only() {
    let (mut store, clock) = store();
    let titled = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    store
        .update_session(
            &titled.id,
            SessionUpdate {
                title: Some("Refactor lexer".to_owned()),
                ..SessionUpdate::default()
            },
        )
        .unwrap();
    clock.set(2_000);
    let chat = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    store
        .update_session(
            &chat.id,
            SessionUpdate {
                title: Some("Untitled".to_owned()),
                ..SessionUpdate::default()
            },
        )
        .unwrap();
    store
        .append_event(&chat.id, user("explain borrowck"))
        .unwrap();
    store
        .append_event(&chat.id, assistant("lifetimes matter"))
        .unwrap();
    store
        .append_event(&chat.id, tool_result("secretoutput"))
        .unwrap();

    assert_eq!(
        ids(&store.list_sessions(Some("lexer")).unwrap()),
        vec![titled.id.clone()]
    );
    assert_eq!(
        ids(&store.list_sessions(Some("borrowck")).unwrap()),
        vec![chat.id.clone()]
    );
    assert_eq!(
        ids(&store.list_sessions(Some("lifetimes")).unwrap()),
        vec![chat.id.clone()]
    );
    assert_eq!(
        ids(&store.list_sessions(Some("lifeti")).unwrap()),
        vec![chat.id.clone()]
    );
    assert!(
        store
            .list_sessions(Some("secretoutput"))
            .unwrap()
            .is_empty()
    );
    assert!(store.list_sessions(Some("hidden")).unwrap().is_empty());
}

#[test]
fn search_treats_query_as_plain_words() {
    let (mut store, _) = store();
    let session = store
        .create_session(new_session(SessionOrigin::Root))
        .unwrap();
    store
        .append_event(&session.id, user("foo bar baz"))
        .unwrap();

    assert!(
        store
            .list_sessions(Some("\"foo\" OR (bar*"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ids(&store.list_sessions(Some("\"foo\" (bar*")).unwrap()),
        vec![session.id.clone()]
    );
    assert!(
        store
            .list_sessions(Some("'; DROP TABLE sessions; --"))
            .unwrap()
            .is_empty()
    );
    assert!(store.list_sessions(Some("NEAR(\"")).unwrap().is_empty());
    assert!(store.list_sessions(Some("*:^")).unwrap().is_empty());
    assert_eq!(store.list_sessions(None).unwrap().len(), 1);
    assert_eq!(store.events(&session.id).unwrap().len(), 1);
}

#[test]
fn reopens_file_with_same_content() {
    let directory =
        std::env::temp_dir().join(format!("session-store-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sessions.sqlite");
    let (session, event) = {
        let mut store = SessionStore::open(&path, || 7_000).unwrap();
        let session = store
            .create_session(new_session(SessionOrigin::Root))
            .unwrap();
        let event = store.append_event(&session.id, user("persist me")).unwrap();
        (store.session(&session.id).unwrap(), event)
    };
    let store = SessionStore::open(&path, || 9_000).unwrap();
    assert_eq!(store.session(&session.id).unwrap(), session);
    assert_eq!(store.events(&session.id).unwrap(), vec![event]);
    assert_eq!(
        ids(&store.list_sessions(Some("persist")).unwrap()),
        vec![session.id]
    );
    drop(store);
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn rejects_newer_schema_version() {
    let directory =
        std::env::temp_dir().join(format!("session-store-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sessions.sqlite");
    rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    let result = SessionStore::open(&path, || 0);
    assert!(matches!(
        result,
        Err(StoreError::UnsupportedSchemaVersion {
            found: 99,
            supported: 1
        })
    ));
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn unknown_session_errors() {
    let (mut store, _) = store();
    let missing = "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
    assert!(matches!(
        store.session(missing),
        Err(StoreError::SessionNotFound(_))
    ));
    assert!(matches!(
        store.events(missing),
        Err(StoreError::SessionNotFound(_))
    ));
    assert!(matches!(
        store.append_event(missing, user("x")),
        Err(StoreError::SessionNotFound(_))
    ));
    assert!(matches!(
        store.update_session(missing, SessionUpdate::default()),
        Err(StoreError::SessionNotFound(_))
    ));
    assert!(matches!(
        store.fork_session(missing, 1),
        Err(StoreError::SessionNotFound(_))
    ));
    assert!(matches!(
        store.delete_session(missing),
        Err(StoreError::SessionNotFound(_))
    ));
}
