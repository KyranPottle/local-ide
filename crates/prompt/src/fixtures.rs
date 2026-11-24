use protocol::{
    Attachment, CompactionKind, Event, EventBody, Mode, ModelProfile, ToolCall, ToolCodec,
    ToolName, WorkspaceSnapshot,
};
use serde_json::Value;

use crate::message::ChatMessage;
use crate::render::PromptInput;
use crate::system_prompt::AgentKind;

pub(crate) const WORKSPACE_TOOLS: [ToolName; 5] = [
    ToolName::Read,
    ToolName::Glob,
    ToolName::Grep,
    ToolName::Edit,
    ToolName::Write,
];

pub(crate) fn profile(tool_codec: ToolCodec) -> ModelProfile {
    ModelProfile {
        tool_codec,
        ..ModelProfile::unprofiled("test-model")
    }
}

// A profile where one character is one token, so a test can state sizes as literals
pub(crate) fn unit_profile(context_budget: u32, reply_reserve: u32) -> ModelProfile {
    ModelProfile {
        context_budget,
        reply_reserve,
        chars_per_token: 1.0,
        ..ModelProfile::unprofiled("test-model")
    }
}

pub(crate) fn agent_input<'a>(profile: &'a ModelProfile, events: &'a [Event]) -> PromptInput<'a> {
    PromptInput {
        profile,
        mode: Mode::Agent,
        capabilities: &WORKSPACE_TOOLS,
        agent_kind: AgentKind::Primary,
        events,
        delegate_targets: &[],
        token_scale: 1.0,
    }
}

pub(crate) fn toolless_input<'a>(
    profile: &'a ModelProfile,
    events: &'a [Event],
) -> PromptInput<'a> {
    PromptInput {
        mode: Mode::Ask,
        capabilities: &[],
        ..agent_input(profile, events)
    }
}

pub(crate) fn user_message(content: &str) -> ChatMessage {
    ChatMessage::User {
        content: content.to_owned(),
    }
}

pub(crate) fn assistant_message(content: &str) -> ChatMessage {
    ChatMessage::Assistant {
        content: content.to_owned(),
        tool_calls: Vec::new(),
    }
}

pub(crate) fn call(id: &str, name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments,
    }
}

fn event(seq: u32, body: EventBody) -> Event {
    Event {
        seq,
        at: 1_795_470_660_000,
        body,
    }
}

pub(crate) fn user(seq: u32, text: &str) -> Event {
    user_with_context(seq, text, Vec::new(), None)
}

pub(crate) fn user_with_rules(
    seq: u32,
    text: &str,
    system_rules: Option<&str>,
    repo_rules: Option<&str>,
) -> Event {
    event(
        seq,
        EventBody::User {
            text: text.to_owned(),
            attachments: Vec::new(),
            workspace: None,
            system_rules: system_rules.map(str::to_owned),
            repo_rules: repo_rules.map(str::to_owned),
        },
    )
}

pub(crate) fn user_with_context(
    seq: u32,
    text: &str,
    attachments: Vec<Attachment>,
    workspace: Option<WorkspaceSnapshot>,
) -> Event {
    event(
        seq,
        EventBody::User {
            text: text.to_owned(),
            attachments,
            workspace,
            system_rules: None,
            repo_rules: None,
        },
    )
}

pub(crate) fn assistant(seq: u32, text: &str, tool_calls: Vec<ToolCall>) -> Event {
    event(
        seq,
        EventBody::Assistant {
            text: text.to_owned(),
            reasoning: "reasoning that the model must not see again".to_owned(),
            tool_calls,
            model: "test-model".to_owned(),
            usage: None,
        },
    )
}

pub(crate) fn tool_result(seq: u32, call_id: &str, name: &str, output: &str) -> Event {
    event(
        seq,
        EventBody::ToolResult {
            call_id: call_id.to_owned(),
            name: name.to_owned(),
            output: output.to_owned(),
            is_error: false,
        },
    )
}

pub(crate) fn summary(seq: u32, through: u32, text: &str) -> Event {
    event(
        seq,
        EventBody::Compaction {
            through,
            kind: CompactionKind::Summary {
                text: text.to_owned(),
                model: "test-model".to_owned(),
            },
        },
    )
}

pub(crate) fn elide(seq: u32, through: u32) -> Event {
    event(
        seq,
        EventBody::Compaction {
            through,
            kind: CompactionKind::Elide,
        },
    )
}

pub(crate) fn model_switch(seq: u32) -> Event {
    event(
        seq,
        EventBody::ModelSwitch {
            from: "test-model".to_owned(),
            to: "other-model".to_owned(),
        },
    )
}
