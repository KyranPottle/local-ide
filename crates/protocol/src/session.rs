use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::tool::ToolName;

// UUID v7 in lowercase hyphenated form
pub type SessionId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Mode {
    Agent,
    Ask,
}

impl Mode {
    pub const fn tools(self) -> &'static [ToolName] {
        match self {
            Self::Agent => &ToolName::ALL,
            Self::Ask => &[ToolName::Read, ToolName::Glob, ToolName::Grep],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum SessionOrigin {
    Root,
    Fork { parent: SessionId, seq: u32 },
    Delegate { parent: SessionId, call_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Session {
    pub id: SessionId,
    pub title: String,
    pub workspace: String,
    pub model: String,
    pub mode: Mode,
    pub origin: SessionOrigin,
    /// Unix time in milliseconds
    #[ts(type = "number")]
    pub created_at: i64,
    /// Unix time in milliseconds
    #[ts(type = "number")]
    pub updated_at: i64,
}
