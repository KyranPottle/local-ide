use std::collections::{BTreeMap, VecDeque};

use serde::Deserialize;

use crate::line_buffer::LineBuffer;
use crate::{
    ChatChunk, ChatCompletion, FinishReason, InferenceError, MALFORMED_EXCERPT_CHARS,
    NativeToolCall, excerpt,
};

pub const MAX_TOOL_CALLS: usize = 64;
pub const MAX_TOOL_ARGUMENT_BYTES: usize = 1024 * 1024;

pub enum Outcome {
    Pending,
    Complete,
}

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<WireUsage>,
    timings: Option<Timings>,
    error: Option<ServerError>,
}

#[derive(Deserialize)]
struct Choice {
    delta: Option<Delta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ToolCallFragment>>,
}

#[derive(Deserialize)]
struct ToolCallFragment {
    index: usize,
    id: Option<String>,
    function: Option<FunctionFragment>,
}

#[derive(Deserialize)]
struct FunctionFragment {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct Timings {
    predicted_per_second: Option<f64>,
}

#[derive(Deserialize)]
struct ServerError {
    #[serde(default)]
    message: String,
}

#[derive(Default)]
pub struct EventDecoder {
    lines: LineBuffer,
    tool_calls: BTreeMap<usize, NativeToolCall>,
    tool_argument_bytes: usize,
    finish: Option<FinishReason>,
    usage: Option<WireUsage>,
    tokens_per_second: Option<f32>,
}

impl EventDecoder {
    pub fn feed(
        &mut self,
        bytes: &[u8],
        out: &mut VecDeque<Result<ChatChunk, InferenceError>>,
    ) -> Result<Outcome, InferenceError> {
        self.lines.extend(bytes);
        while let Some(line) = self.lines.next_line()? {
            if let Outcome::Complete = self.process_line(&line, out)? {
                return Ok(Outcome::Complete);
            }
        }
        Ok(Outcome::Pending)
    }

    fn process_line(
        &mut self,
        line: &[u8],
        out: &mut VecDeque<Result<ChatChunk, InferenceError>>,
    ) -> Result<Outcome, InferenceError> {
        // Blank lines, comments and the fields event, id and retry carry nothing for a chat completion
        let Some(payload) = line.strip_prefix(b"data:") else {
            return Ok(Outcome::Pending);
        };
        let payload = payload.strip_prefix(b" ").unwrap_or(payload);
        if payload == b"[DONE]" {
            out.push_back(Ok(ChatChunk::Finished(self.completion()?)));
            return Ok(Outcome::Complete);
        }
        let chunk: StreamChunk = serde_json::from_slice(payload).map_err(|_| {
            InferenceError::MalformedJson(excerpt(payload, MALFORMED_EXCERPT_CHARS))
        })?;
        if let Some(error) = chunk.error {
            return Err(InferenceError::Server(error.message));
        }
        if let Some(usage) = chunk.usage {
            self.usage = Some(usage);
        }
        if let Some(speed) = chunk
            .timings
            .and_then(|timings| timings.predicted_per_second)
        {
            self.tokens_per_second = Some(speed as f32);
        }
        let Some(choice) = chunk.choices.into_iter().next() else {
            return Ok(Outcome::Pending);
        };
        if let Some(reason) = choice.finish_reason {
            self.finish = Some(FinishReason::from_wire(reason));
        }
        let Some(delta) = choice.delta else {
            return Ok(Outcome::Pending);
        };
        if let Some(text) = delta.reasoning_content.filter(|text| !text.is_empty()) {
            out.push_back(Ok(ChatChunk::Reasoning(text)));
        }
        if let Some(text) = delta.content.filter(|text| !text.is_empty()) {
            out.push_back(Ok(ChatChunk::Text(text)));
        }
        for fragment in delta.tool_calls.unwrap_or_default() {
            self.apply_tool_fragment(fragment)?;
        }
        Ok(Outcome::Pending)
    }

    fn apply_tool_fragment(&mut self, fragment: ToolCallFragment) -> Result<(), InferenceError> {
        if !self.tool_calls.contains_key(&fragment.index) && self.tool_calls.len() >= MAX_TOOL_CALLS
        {
            return Err(InferenceError::LimitExceeded {
                limit: "tool calls",
                max: MAX_TOOL_CALLS,
            });
        }
        let call = self.tool_calls.entry(fragment.index).or_default();
        if let Some(id) = fragment.id.filter(|id| !id.is_empty()) {
            call.id = id;
        }
        let Some(function) = fragment.function else {
            return Ok(());
        };
        if let Some(name) = function.name.filter(|name| !name.is_empty()) {
            call.name = name;
        }
        let Some(arguments) = function.arguments else {
            return Ok(());
        };
        self.tool_argument_bytes += arguments.len();
        if self.tool_argument_bytes > MAX_TOOL_ARGUMENT_BYTES {
            return Err(InferenceError::LimitExceeded {
                limit: "tool argument bytes",
                max: MAX_TOOL_ARGUMENT_BYTES,
            });
        }
        call.arguments.push_str(&arguments);
        Ok(())
    }

    fn completion(&mut self) -> Result<ChatCompletion, InferenceError> {
        let finish = self.finish.take().ok_or(InferenceError::Truncated)?;
        Ok(ChatCompletion {
            tool_calls: std::mem::take(&mut self.tool_calls).into_values().collect(),
            finish,
            usage: self.usage.take().map(|usage| protocol::Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                tokens_per_second: self.tokens_per_second,
            }),
        })
    }
}
