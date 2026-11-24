use inference::{ChatRequest, InferenceClient};
use prompt::{
    AgentKind, ChatMessage, ChatToolCall, ParsedReply, PromptInput, offered_tools, parse_reply,
    render,
};
use protocol::{
    Event, EventBody, Mode, ModelProfile, ToolCall, ToolCodec, ToolName, WorkspaceSnapshot,
};

use super::exchange::{ExchangeError, Reply, exchange};
use super::record::{CharsPerToken, TaskRun};
use crate::run::{chat_request, problem_calls};

const LOAD_PROMPT: &str = "Reply with the single word ready.";
const LOAD_MAX_TOKENS: u32 = 16;
const READ_TASK: &str = "Read the file README.md.";
const READ_PATH: &str = "README.md";
const EDIT_TASK: &str = "In src/answer.ts change the number 41 to 42.";
const ANSWER_PATH: &str = "src/answer.ts";
const ANSWER_TEXT: &str = "export const answer = 41";
const ANSWER_GOAL: &str = "export const answer = 42";
const ANSWER_READ_OUTPUT: &str = "1|export const answer = 41";
const GLOB_OUTPUT: &str = "README.md\nsrc/answer.ts";
const GREP_OUTPUT: &str = "src/answer.ts:1|export const answer = 41";
const DELEGATE_OUTPUT: &str = "error: delegate is not available in this task";
const TASK_REQUEST_LIMIT: usize = 6;
const NO_FINAL_ANSWER: &str = "no final answer after 6 model requests";
const EARLY_FINISH: &str = "the model finished before a correct edit";
const EMPTY_FINISH: &str = "the model finished with no text";

// The tail difference cancels the chat template, so only the tail length enters the ratio
const SOURCE_HEAD: &str = r"export function parseDuration(text: string): number {
  const match = /^(\d+)(ms|s|m)$/.exec(text.trim())
  if (!match) throw new Error(`invalid duration: ${text}`)
  const value = Number(match[1])
  switch (match[2]) {
    case 'ms':
      return value
    case 's':
      return value * 1000
    default:
      return value * 60_000
  }
}
";
const SOURCE_TAIL: &str = r"
export function groupBy<T, K extends string>(items: T[], key: (item: T) => K): Record<K, T[]> {
  const groups = {} as Record<K, T[]>
  for (const item of items) {
    const name = key(item)
    const group = groups[name] ?? []
    group.push(item)
    groups[name] = group
  }
  return groups
}

export async function retry<T>(attempts: number, operation: () => Promise<T>): Promise<T> {
  let failure: unknown
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await operation()
    } catch (error) {
      failure = error
      await new Promise((resolve) => setTimeout(resolve, 100 * attempt))
    }
  }
  throw failure
}
";

pub(super) struct LoadMeasurement {
    pub seconds: f64,
    pub tokens_per_second: Option<f32>,
}

fn plain_request(
    profile: &ModelProfile,
    text: &str,
    max_tokens: u32,
) -> Result<ChatRequest, ExchangeError> {
    let messages = [ChatMessage::User {
        content: text.to_owned(),
    }];
    Ok(ChatRequest {
        model: profile.id.clone(),
        messages: serde_json::to_value(messages)
            .map_err(|error| ExchangeError::Unavailable(error.to_string()))?,
        tools: None,
        max_tokens,
        temperature: profile.temperature,
        top_p: profile.top_p,
    })
}

pub(super) async fn measure_load(
    client: &InferenceClient,
    profile: &ModelProfile,
) -> Result<LoadMeasurement, ExchangeError> {
    let request = plain_request(profile, LOAD_PROMPT, LOAD_MAX_TOKENS)?;
    let reply = exchange(client, profile, request, "call_load_").await?;
    Ok(LoadMeasurement {
        seconds: reply.first_byte.as_secs_f64(),
        tokens_per_second: reply.usage.and_then(|usage| usage.tokens_per_second),
    })
}

pub(super) async fn measure_chars_per_token(
    client: &InferenceClient,
    profile: &ModelProfile,
) -> Result<CharsPerToken, ExchangeError> {
    let long_text = format!("{SOURCE_HEAD}{SOURCE_TAIL}");
    let mut prompt_tokens = Vec::new();
    for text in [SOURCE_HEAD, long_text.as_str()] {
        let request = plain_request(profile, text, 1)?;
        let reply = exchange(client, profile, request, "call_count_").await?;
        let Some(usage) = reply.usage else {
            return Ok(CharsPerToken::NotReported);
        };
        prompt_tokens.push(usage.prompt_tokens);
    }
    let [short, long] = prompt_tokens[..] else {
        unreachable!("the loop pushes one count for each of the two texts")
    };
    if long <= short {
        return Ok(CharsPerToken::NotIncreasing { short, long });
    }
    let chars = SOURCE_TAIL.chars().count() as f32;
    Ok(CharsPerToken::Measured {
        value: chars / (long - short) as f32,
    })
}

fn probe_workspace() -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        name: "calibration".to_owned(),
        entries: vec!["src/".to_owned(), READ_PATH.to_owned()],
        active_file: None,
    }
}

fn workspace_capabilities() -> Vec<ToolName> {
    ToolName::ALL
        .into_iter()
        .filter(|tool| tool.is_workspace_tool())
        .collect()
}

struct Turn {
    // Stream text before the parse, the evidence for a call that did not parse
    text: String,
    reply: ParsedReply,
    native_calls: Vec<ChatToolCall>,
    call_id_prefix: String,
}

struct ProbeSession {
    profile: ModelProfile,
    capabilities: Vec<ToolName>,
    offered: Vec<ToolName>,
    events: Vec<Event>,
}

impl ProbeSession {
    fn new(profile: &ModelProfile, codec: ToolCodec, task: &str) -> Self {
        let capabilities = workspace_capabilities();
        let mut session = Self {
            profile: ModelProfile {
                tool_codec: codec,
                ..profile.clone()
            },
            offered: offered_tools(Mode::Agent, &capabilities, AgentKind::Primary),
            capabilities,
            events: Vec::new(),
        };
        session.append(EventBody::User {
            text: task.to_owned(),
            attachments: Vec::new(),
            workspace: Some(probe_workspace()),
            system_rules: None,
            repo_rules: None,
        });
        session
    }

    fn append(&mut self, body: EventBody) {
        let seq = u32::try_from(self.events.len()).map_or(u32::MAX, |count| count + 1);
        self.events.push(Event { seq, at: 0, body });
    }

    fn call_id_prefix(&self) -> String {
        format!("call_{}_", self.events.len() + 1)
    }

    async fn request(&self, client: &InferenceClient) -> Result<Turn, ExchangeError> {
        let rendered = render(&PromptInput {
            profile: &self.profile,
            mode: Mode::Agent,
            capabilities: &self.capabilities,
            agent_kind: AgentKind::Primary,
            events: &self.events,
            delegate_targets: &[],
            token_scale: 1.0,
        })
        .map_err(|error| ExchangeError::Unavailable(error.to_string()))?;
        let request = chat_request(&self.profile, &rendered)
            .map_err(|error| ExchangeError::Unavailable(error.to_string()))?;
        let prefix = self.call_id_prefix();
        let Reply {
            text, native_calls, ..
        } = exchange(client, &self.profile, request, &prefix).await?;
        let reply = parse_reply(self.profile.tool_codec, &text, &native_calls, &prefix);
        Ok(Turn {
            text,
            reply,
            native_calls,
            call_id_prefix: prefix,
        })
    }

    fn is_read_of(&self, call: &ToolCall, path: &str) -> bool {
        prompt::validate_tool_call(call, &self.offered) == Ok(ToolName::Read)
            && call.arguments["path"] == path
    }
}

fn reply_summary(text: &str, reply: &ParsedReply) -> String {
    if let Some(problem) = reply.problems.first() {
        return format!("{problem} reply text: {text:?}");
    }
    if reply.tool_calls.is_empty() {
        return format!("no tool call, reply text: {:?}", reply.text);
    }
    let calls: Vec<String> = reply
        .tool_calls
        .iter()
        .map(|call| format!("{} {}", call.name, call.arguments))
        .collect();
    format!("tool calls: {}", calls.join("; "))
}

// Ok(Err(detail)) is a failed probe, and Err means that the probe could not run
pub(super) async fn probe_tool_call(
    client: &InferenceClient,
    profile: &ModelProfile,
    codec: ToolCodec,
) -> Result<Result<(), String>, String> {
    let session = ProbeSession::new(profile, codec, READ_TASK);
    let Turn { text, reply, .. } = match session.request(client).await {
        Ok(turn) => turn,
        Err(ExchangeError::Rejected(detail)) => return Ok(Err(detail)),
        Err(ExchangeError::Unavailable(detail)) => return Err(detail),
    };
    let passed = match codec {
        ToolCodec::Native => reply
            .tool_calls
            .iter()
            .any(|call| session.is_read_of(call, READ_PATH)),
        ToolCodec::Text => {
            reply.problems.is_empty()
                && matches!(&reply.tool_calls[..], [call] if session.is_read_of(call, READ_PATH))
        }
    };
    if passed {
        return Ok(Ok(()));
    }
    Ok(Err(reply_summary(&text, &reply)))
}

struct ToolAnswer {
    output: String,
    is_error: bool,
}

impl ToolAnswer {
    fn success(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: false,
        }
    }

    fn error(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            is_error: true,
        }
    }
}

fn argument<'a>(call: &'a ToolCall, name: &str) -> &'a str {
    call.arguments[name].as_str().unwrap_or_default()
}

// Plays the browser over one fixed file, and records the task progress in run
fn answer_call(session: &ProbeSession, call: &ToolCall, run: &mut TaskRun) -> ToolAnswer {
    let tool = match prompt::validate_tool_call(call, &session.offered) {
        Ok(tool) => tool,
        Err(error) => return ToolAnswer::error(error.to_string()),
    };
    let path = argument(call, "path");
    match tool {
        ToolName::Glob => ToolAnswer::success(GLOB_OUTPUT),
        ToolName::Grep => ToolAnswer::success(GREP_OUTPUT),
        ToolName::Delegate => ToolAnswer::error(DELEGATE_OUTPUT),
        ToolName::Write => ToolAnswer::success(format!("wrote {path} (1 lines)")),
        ToolName::Read if path == ANSWER_PATH => {
            run.reached_read = true;
            ToolAnswer::success(ANSWER_READ_OUTPUT)
        }
        ToolName::Read => ToolAnswer::error(format!("error: file not found: {path}")),
        ToolName::Edit if path != ANSWER_PATH => {
            ToolAnswer::error(format!("error: file not found: {path}"))
        }
        ToolName::Edit => edit_answer(argument(call, "old"), argument(call, "new"), run),
    }
}

fn edit_answer(old: &str, new: &str, run: &mut TaskRun) -> ToolAnswer {
    if old.is_empty() {
        return ToolAnswer::error("error: old text is empty");
    }
    if old == new {
        return ToolAnswer::error("error: old and new are the same");
    }
    match ANSWER_TEXT.matches(old).count() {
        0 => ToolAnswer::error(format!("error: old text not found in {ANSWER_PATH}")),
        1 => {
            let result = ANSWER_TEXT.replacen(old, new, 1);
            if result == ANSWER_GOAL {
                run.correct_edit = true;
            } else {
                run.note_failure(format!("the edit gave {result:?}"));
            }
            ToolAnswer::success(format!("edited {ANSWER_PATH}: 1 replacement(s)"))
        }
        count => ToolAnswer::error(format!(
            "error: old text matches {count} places in {ANSWER_PATH}. Add surrounding lines or set all=true."
        )),
    }
}

// Plays the tool host for the edit task, Err means that the task could not run
pub(super) async fn run_edit_task(
    client: &InferenceClient,
    profile: &ModelProfile,
    codec: ToolCodec,
) -> Result<TaskRun, String> {
    let mut session = ProbeSession::new(profile, codec, EDIT_TASK);
    let mut run = TaskRun::default();
    for _ in 0..TASK_REQUEST_LIMIT {
        let Turn {
            reply,
            native_calls,
            call_id_prefix,
            ..
        } = match session.request(client).await {
            Ok(turn) => turn,
            Err(ExchangeError::Rejected(detail)) => {
                run.note_failure(detail);
                return Ok(run);
            }
            Err(ExchangeError::Unavailable(detail)) => return Err(detail),
        };
        let failed_calls = problem_calls(
            reply.problems.clone(),
            &native_calls,
            &reply.tool_calls,
            &call_id_prefix,
        );
        session.append(EventBody::Assistant {
            text: reply.text.clone(),
            reasoning: String::new(),
            tool_calls: reply
                .tool_calls
                .iter()
                .chain(failed_calls.iter().map(|(call, _)| call))
                .cloned()
                .collect(),
            model: profile.id.clone(),
            usage: None,
        });
        for (call, problem) in &failed_calls {
            run.note_failure(problem.clone());
            session.append(EventBody::ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                output: problem.clone(),
                is_error: true,
            });
        }
        if reply.tool_calls.is_empty() && failed_calls.is_empty() {
            match (run.correct_edit, reply.text.is_empty()) {
                (true, false) => run.finished = true,
                (true, true) => run.note_failure(EMPTY_FINISH.to_owned()),
                (false, _) => run.note_failure(EARLY_FINISH.to_owned()),
            }
            return Ok(run);
        }
        for call in &reply.tool_calls {
            let answer = answer_call(&session, call, &mut run);
            if answer.is_error {
                run.note_failure(answer.output.clone());
            }
            session.append(EventBody::ToolResult {
                call_id: call.id.clone(),
                name: call.name.clone(),
                output: answer.output,
                is_error: answer.is_error,
            });
        }
    }
    run.note_failure(NO_FINAL_ANSWER.to_owned());
    Ok(run)
}
