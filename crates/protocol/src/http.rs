use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::event::Event;
use crate::session::{Mode, Session, SessionId};
use crate::websocket::ContextReport;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EngineStatus {
    pub version: String,
    pub active_run: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CreateSession {
    pub workspace: String,
    pub model: String,
    pub mode: Mode,
}

/// An absent field and a null field both keep the stored value
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct UpdateSession {
    pub title: Option<String>,
    pub model: Option<String>,
    pub mode: Option<Mode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ForkSession {
    /// The new session holds the events with a seq at or below this value
    pub seq: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SessionDetail {
    pub session: Session,
    pub events: Vec<Event>,
    /// Estimate for the session model with each workspace tool of the mode, plus Delegate for a primary agent session
    pub context: ContextReport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ApiError {
    pub message: String,
}
