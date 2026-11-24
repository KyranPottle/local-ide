// Each function is pure: the same input gives the same output, byte for byte

mod compaction;
mod conversation;
mod delimiter;
mod estimate;
mod message;
mod render;
mod reply;
mod stream;
mod system_prompt;
mod text_codec;
mod tool_catalog;
mod truncation;
mod validation;

#[cfg(test)]
mod fixtures;

pub use compaction::{CompactionPlan, plan_compaction};
pub use estimate::calibrate_token_scale;
pub use message::{ChatMessage, ChatToolCall};
pub use render::{PromptInput, RenderError, RenderedPrompt, context_report, render};
pub use reply::{ParsedReply, parse_reply};
pub use stream::{SplitStream, StreamSegment, StreamSplitter};
pub use system_prompt::AgentKind;
pub use tool_catalog::{ToolDeclaration, offered_tools};
pub use truncation::cap_output;
pub use validation::{ToolCallError, validate_tool_call};
