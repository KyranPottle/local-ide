use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::event::{Attachment, Event, WorkspaceSnapshot};
use crate::session::{Session, SessionId};
use crate::tool::{ToolCall, ToolName};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ClientFrame {
    RunStart {
        session_id: SessionId,
        text: String,
        attachments: Vec<Attachment>,
        workspace: WorkspaceSnapshot,
        /// Tools that this client can execute
        capabilities: Vec<ToolName>,
        /// Text of AGENTS.md at the workspace root, null when the file is absent
        #[serde(default)]
        repo_rules: Option<String>,
    },
    RunCancel,
    ToolResult {
        call_id: String,
        output: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DeltaChannel {
    Text,
    Reasoning,
}

/// Each field is a count of estimated tokens
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ContextReport {
    pub budget: u32,
    pub reserve: u32,
    pub system: u32,
    pub tools: u32,
    pub history: u32,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum RunOutcome {
    Complete,
    Cancelled,
    StepLimit,
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum RejectReason {
    Busy { session_id: SessionId },
    InvalidRequest { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ServerFrame {
    RunStarted {
        session_id: SessionId,
    },
    Delta {
        session_id: SessionId,
        channel: DeltaChannel,
        text: String,
    },
    // The client executes the workspace tool and answers with ClientFrame::ToolResult
    ToolCall {
        session_id: SessionId,
        call: ToolCall,
    },
    // An event that the engine persisted, authoritative over the deltas before it
    Event {
        session_id: SessionId,
        event: Event,
    },
    ContextReport {
        session_id: SessionId,
        /// Model that the next request uses
        model: String,
        report: ContextReport,
    },
    SessionChanged {
        session: Session,
    },
    RunFinished {
        session_id: SessionId,
        outcome: RunOutcome,
    },
    Rejected {
        reason: RejectReason,
    },
}
