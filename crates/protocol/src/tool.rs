use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ToolName {
    Read,
    Glob,
    Grep,
    Edit,
    Write,
    Delegate,
}

impl ToolName {
    pub const ALL: [Self; 6] = [
        Self::Read,
        Self::Glob,
        Self::Grep,
        Self::Edit,
        Self::Write,
        Self::Delegate,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Glob => "glob",
            Self::Grep => "grep",
            Self::Edit => "edit",
            Self::Write => "write",
            Self::Delegate => "delegate",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.as_str() == name)
    }

    // The browser executes a workspace tool and the engine executes each other tool
    pub const fn is_workspace_tool(self) -> bool {
        match self {
            Self::Read | Self::Glob | Self::Grep | Self::Edit | Self::Write => true,
            Self::Delegate => false,
        }
    }

    pub const fn mutates_workspace(self) -> bool {
        match self {
            Self::Edit | Self::Write => true,
            Self::Read | Self::Glob | Self::Grep | Self::Delegate => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ToolCall {
    pub id: String,
    /// A model can emit an unknown name, so this stays a string
    pub name: String,
    pub arguments: serde_json::Value,
}
