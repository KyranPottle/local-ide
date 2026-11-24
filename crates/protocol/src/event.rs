use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::tool::ToolCall;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    /// Generation speed that the inference server reports
    pub tokens_per_second: Option<f32>,
}

/// Line numbers start at 1 and both ends are inclusive
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LineRange {
    pub first: u32,
    pub last: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Attachment {
    pub path: String,
    pub lines: Option<LineRange>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WorkspaceSnapshot {
    pub name: String,
    /// Top-level names of the workspace, a directory ends with `/`, at most 60
    pub entries: Vec<String>,
    pub active_file: Option<String>,
}

/// Changes the model view only, the store keeps each event
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum CompactionKind {
    // Each tool result with a seq at or below through renders as a one-line stub
    Elide,
    // The model sees text in place of each event with a seq at or below through
    Summary { text: String, model: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum EventBody {
    User {
        text: String,
        attachments: Vec<Attachment>,
        /// Present on the first user event of a session and when the snapshot changed
        workspace: Option<WorkspaceSnapshot>,
        /// Engine configuration text, set on the first user event only
        #[serde(default)]
        system_rules: Option<String>,
        /// Text of AGENTS.md, set on the first user event only
        #[serde(default)]
        repo_rules: Option<String>,
    },
    Assistant {
        text: String,
        /// Never returns to the model
        reasoning: String,
        tool_calls: Vec<ToolCall>,
        model: String,
        usage: Option<Usage>,
    },
    ToolResult {
        call_id: String,
        name: String,
        output: String,
        is_error: bool,
    },
    Compaction {
        through: u32,
        kind: CompactionKind,
    },
    ModelSwitch {
        from: String,
        to: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Event {
    /// Starts at 1 and increases by 1 inside one session
    pub seq: u32,
    /// Unix time in milliseconds
    #[ts(type = "number")]
    pub at: i64,
    pub body: EventBody,
}
