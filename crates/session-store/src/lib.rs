use std::path::Path;

use protocol::{Event, EventBody, Mode, Session, SessionId, SessionOrigin};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

const SCHEMA_VERSION: i64 = 1;
const TITLE_LENGTH: usize = 60;

// Integer keys give the FTS tables a stable rowid, an implicit rowid can change on VACUUM
const SCHEMA: &str = "
CREATE TABLE sessions (
    key INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    workspace TEXT NOT NULL,
    model TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('agent', 'ask')),
    origin_kind TEXT NOT NULL CHECK (origin_kind IN ('root', 'fork', 'delegate')),
    parent_id TEXT,
    fork_seq INTEGER,
    delegate_call_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK (
        (origin_kind = 'root' AND parent_id IS NULL AND fork_seq IS NULL AND delegate_call_id IS NULL)
        OR (origin_kind = 'fork' AND parent_id IS NOT NULL AND fork_seq IS NOT NULL AND delegate_call_id IS NULL)
        OR (origin_kind = 'delegate' AND parent_id IS NOT NULL AND fork_seq IS NULL AND delegate_call_id IS NOT NULL)
    )
);
CREATE INDEX sessions_parent ON sessions (parent_id);
CREATE INDEX sessions_updated ON sessions (updated_at);

CREATE TABLE events (
    key INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    at INTEGER NOT NULL,
    body TEXT NOT NULL,
    UNIQUE (session_id, seq)
);

CREATE VIRTUAL TABLE title_index USING fts5 (title);
CREATE VIRTUAL TABLE event_index USING fts5 (text);

CREATE TRIGGER sessions_title_insert AFTER INSERT ON sessions BEGIN
    INSERT INTO title_index (rowid, title) VALUES (new.key, new.title);
END;
CREATE TRIGGER sessions_title_update AFTER UPDATE OF title ON sessions BEGIN
    UPDATE title_index SET title = new.title WHERE rowid = new.key;
END;
CREATE TRIGGER sessions_title_delete AFTER DELETE ON sessions BEGIN
    DELETE FROM title_index WHERE rowid = old.key;
END;
CREATE TRIGGER events_text_delete AFTER DELETE ON events BEGIN
    DELETE FROM event_index WHERE rowid = old.key;
END;
";

const SESSION_COLUMNS: &str = "id, title, workspace, model, mode, origin_kind, parent_id, fork_seq, delegate_call_id, created_at, updated_at";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("session {0} not found")]
    SessionNotFound(SessionId),
    #[error("sequence {seq} is outside the event log of session {session}")]
    InvalidSequence { session: SessionId, seq: u32 },
    #[error("schema version {found} is newer than the supported version {supported}")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },
    #[error("stored event body does not decode: {0}")]
    BodyDecode(#[from] serde_json::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

pub struct NewSession {
    pub workspace: String,
    pub model: String,
    pub mode: Mode,
    pub origin: SessionOrigin,
}

#[derive(Default)]
pub struct SessionUpdate {
    pub title: Option<String>,
    pub model: Option<String>,
    pub mode: Option<Mode>,
}

pub struct SessionStore {
    connection: Connection,
    clock: Box<dyn Fn() -> i64 + Send>,
}

impl SessionStore {
    pub fn open(path: &Path, clock: impl Fn() -> i64 + Send + 'static) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        connection.pragma_update_and_check(None, "journal_mode", "wal", |_| Ok(()))?;
        Self::initialize(connection, Box::new(clock))
    }

    pub fn open_in_memory(clock: impl Fn() -> i64 + Send + 'static) -> Result<Self, StoreError> {
        Self::initialize(Connection::open_in_memory()?, Box::new(clock))
    }

    fn initialize(
        mut connection: Connection,
        clock: Box<dyn Fn() -> i64 + Send>,
    ) -> Result<Self, StoreError> {
        connection.pragma_update(None, "foreign_keys", true)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        match version {
            0 => {
                let transaction = connection.transaction()?;
                transaction.execute_batch(SCHEMA)?;
                transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                transaction.commit()?;
            }
            SCHEMA_VERSION => {}
            found => {
                return Err(StoreError::UnsupportedSchemaVersion {
                    found,
                    supported: SCHEMA_VERSION,
                });
            }
        }
        Ok(Self { connection, clock })
    }

    pub fn create_session(&mut self, new: NewSession) -> Result<Session, StoreError> {
        let now = (self.clock)();
        let session = Session {
            id: uuid::Uuid::now_v7().to_string(),
            title: String::new(),
            workspace: new.workspace,
            model: new.model,
            mode: new.mode,
            origin: new.origin,
            created_at: now,
            updated_at: now,
        };
        insert_session(&self.connection, &session)?;
        Ok(session)
    }

    pub fn session(&self, id: &str) -> Result<Session, StoreError> {
        read_session(&self.connection, id)
    }

    pub fn list_sessions(&self, query: Option<&str>) -> Result<Vec<Session>, StoreError> {
        let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
            let mut statement = self.connection.prepare(&format!(
                "SELECT {SESSION_COLUMNS} FROM sessions ORDER BY updated_at DESC, key DESC"
            ))?;
            return Ok(statement
                .query_map([], session_from_row)?
                .collect::<Result<_, _>>()?);
        };
        let words = search_words(query);
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self.connection.prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM sessions
             WHERE key IN (SELECT rowid FROM title_index WHERE title_index MATCH ?1)
                OR id IN (SELECT events.session_id FROM event_index
                          JOIN events ON events.key = event_index.rowid
                          WHERE event_index MATCH ?1)
             ORDER BY updated_at DESC, key DESC"
        ))?;
        let rows = statement.query_map([fts_expression(&words)], session_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    // Applies each present field, a changed model also appends a ModelSwitch event
    pub fn update_session(
        &mut self,
        id: &str,
        update: SessionUpdate,
    ) -> Result<Session, StoreError> {
        let now = (self.clock)();
        let transaction = self.connection.transaction()?;
        let mut session = read_session(&transaction, id)?;
        if let Some(model) = update.model.filter(|model| *model != session.model) {
            let from = std::mem::replace(&mut session.model, model.clone());
            append(
                &transaction,
                &mut session,
                now,
                EventBody::ModelSwitch { from, to: model },
            )?;
        }
        if let Some(title) = update.title {
            session.title = title;
        }
        if let Some(mode) = update.mode {
            session.mode = mode;
        }
        session.updated_at = now;
        transaction.execute(
            "UPDATE sessions SET title = ?2, model = ?3, mode = ?4, updated_at = ?5 WHERE id = ?1",
            params![
                id,
                session.title,
                session.model,
                mode_name(session.mode),
                now
            ],
        )?;
        transaction.commit()?;
        Ok(session)
    }

    // Deletes the session and its Delegate descendants, Fork children stay
    pub fn delete_session(&mut self, id: &str) -> Result<(), StoreError> {
        let deleted = self.connection.execute(
            "WITH RECURSIVE removed (id) AS (
                 SELECT ?1
                 UNION SELECT sessions.id FROM sessions
                       JOIN removed ON sessions.parent_id = removed.id
                       WHERE sessions.origin_kind = 'delegate'
             )
             DELETE FROM sessions WHERE id IN removed",
            [id],
        )?;
        if deleted == 0 {
            return Err(StoreError::SessionNotFound(id.to_owned()));
        }
        Ok(())
    }

    pub fn append_event(&mut self, id: &str, body: EventBody) -> Result<Event, StoreError> {
        let now = (self.clock)();
        let transaction = self.connection.transaction()?;
        let mut session = read_session(&transaction, id)?;
        let event = append(&transaction, &mut session, now, body)?;
        transaction.execute(
            "UPDATE sessions SET title = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, session.title, now],
        )?;
        transaction.commit()?;
        Ok(event)
    }

    pub fn events(&self, id: &str) -> Result<Vec<Event>, StoreError> {
        read_session(&self.connection, id)?;
        let mut statement = self
            .connection
            .prepare("SELECT seq, at, body FROM events WHERE session_id = ?1 ORDER BY seq")?;
        let rows = statement.query_map([id], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (seq, at, body) = row?;
            Ok(Event {
                seq,
                at,
                body: serde_json::from_str(&body)?,
            })
        })
        .collect()
    }

    pub fn fork_session(&mut self, id: &str, seq: u32) -> Result<Session, StoreError> {
        let now = (self.clock)();
        let transaction = self.connection.transaction()?;
        let parent = read_session(&transaction, id)?;
        if seq == 0 || seq > last_seq(&transaction, id)? {
            return Err(StoreError::InvalidSequence {
                session: id.to_owned(),
                seq,
            });
        }
        let fork = Session {
            id: uuid::Uuid::now_v7().to_string(),
            title: parent.title,
            workspace: parent.workspace,
            model: parent.model,
            mode: parent.mode,
            origin: SessionOrigin::Fork {
                parent: parent.id,
                seq,
            },
            created_at: now,
            updated_at: now,
        };
        insert_session(&transaction, &fork)?;
        transaction.execute(
            "INSERT INTO events (session_id, seq, at, body)
             SELECT ?1, seq, at, body FROM events WHERE session_id = ?2 AND seq <= ?3 ORDER BY seq",
            params![fork.id, id, seq],
        )?;
        transaction.execute(
            "INSERT INTO event_index (rowid, text)
             SELECT copy.key, event_index.text FROM events AS copy
             JOIN events AS source ON source.session_id = ?2 AND source.seq = copy.seq
             JOIN event_index ON event_index.rowid = source.key
             WHERE copy.session_id = ?1",
            params![fork.id, id],
        )?;
        transaction.commit()?;
        Ok(fork)
    }
}

fn insert_session(connection: &Connection, session: &Session) -> Result<(), StoreError> {
    let (kind, parent, fork_seq, call_id) = match &session.origin {
        SessionOrigin::Root => ("root", None, None, None),
        SessionOrigin::Fork { parent, seq } => ("fork", Some(parent), Some(*seq), None),
        SessionOrigin::Delegate { parent, call_id } => {
            ("delegate", Some(parent), None, Some(call_id))
        }
    };
    connection.execute(
        &format!("INSERT INTO sessions ({SESSION_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"),
        params![
            session.id,
            session.title,
            session.workspace,
            session.model,
            mode_name(session.mode),
            kind,
            parent,
            fork_seq,
            call_id,
            session.created_at,
            session.updated_at,
        ],
    )?;
    Ok(())
}

fn read_session(connection: &Connection, id: &str) -> Result<Session, StoreError> {
    connection
        .query_row(
            &format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id = ?1"),
            [id],
            session_from_row,
        )
        .optional()?
        .ok_or_else(|| StoreError::SessionNotFound(id.to_owned()))
}

fn last_seq(transaction: &Transaction, id: &str) -> Result<u32, StoreError> {
    Ok(transaction.query_row(
        "SELECT COALESCE(MAX(seq), 0) FROM events WHERE session_id = ?1",
        [id],
        |row| row.get(0),
    )?)
}

// Writes the event and applies the default title to session, the caller persists the session row
fn append(
    transaction: &Transaction,
    session: &mut Session,
    at: i64,
    body: EventBody,
) -> Result<Event, StoreError> {
    let seq = last_seq(transaction, &session.id)? + 1;
    transaction.execute(
        "INSERT INTO events (session_id, seq, at, body) VALUES (?1, ?2, ?3, ?4)",
        params![session.id, seq, at, serde_json::to_string(&body)?],
    )?;
    if let Some(text) = searchable_text(&body) {
        transaction.execute(
            "INSERT INTO event_index (rowid, text) VALUES (?1, ?2)",
            params![transaction.last_insert_rowid(), text],
        )?;
    }
    if let EventBody::User { text, .. } = &body
        && session.title.is_empty()
    {
        session.title = default_title(text);
    }
    session.updated_at = at;
    Ok(Event { seq, at, body })
}

fn searchable_text(body: &EventBody) -> Option<&str> {
    match body {
        EventBody::User { text, .. } | EventBody::Assistant { text, .. } => Some(text),
        EventBody::ToolResult { .. }
        | EventBody::Compaction { .. }
        | EventBody::ModelSwitch { .. } => None,
    }
}

fn default_title(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or_default().trim();
    first_line
        .chars()
        .take(TITLE_LENGTH)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

// Matches the word split of the FTS5 unicode61 tokenizer so that no user character reaches FTS syntax
fn search_words(query: &str) -> Vec<&str> {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect()
}

fn fts_expression(words: &[&str]) -> String {
    let phrases: Vec<String> = words.iter().map(|word| format!("\"{word}\"")).collect();
    format!("{}*", phrases.join(" "))
}

const fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Agent => "agent",
        Mode::Ask => "ask",
    }
}

fn session_from_row(row: &Row) -> rusqlite::Result<Session> {
    let mode = match row.get_ref(4)?.as_str()? {
        "agent" => Mode::Agent,
        "ask" => Mode::Ask,
        other => return Err(invalid_column(4, "mode", other)),
    };
    let origin = match row.get_ref(5)?.as_str()? {
        "root" => SessionOrigin::Root,
        "fork" => SessionOrigin::Fork {
            parent: row.get(6)?,
            seq: row.get(7)?,
        },
        "delegate" => SessionOrigin::Delegate {
            parent: row.get(6)?,
            call_id: row.get(8)?,
        },
        other => return Err(invalid_column(5, "origin_kind", other)),
    };
    Ok(Session {
        id: row.get(0)?,
        title: row.get(1)?,
        workspace: row.get(2)?,
        model: row.get(3)?,
        mode,
        origin,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn invalid_column(index: usize, column: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        format!("unknown {column} value {value:?}").into(),
    )
}
