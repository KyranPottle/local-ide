use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures_util::StreamExt;
use inference::{ChatChunk, ChatRequest, NativeToolCall};
use prompt::{
    AgentKind, ChatMessage, ChatToolCall, CompactionPlan, PromptInput, RenderedPrompt,
    StreamSplitter, calibrate_token_scale, cap_output, offered_tools, parse_reply, plan_compaction,
    render, validate_tool_call,
};
use protocol::{
    Attachment, CompactionKind, DeltaChannel, Event, EventBody, ModelProfile,
    RULES_CHARACTER_LIMIT, RunOutcome, ServerFrame, Session, SessionId, SessionOrigin, ToolCall,
    ToolName, Usage, WorkspaceSnapshot, rules_text,
};
use serde::Deserialize;
use serde_json::Value;
use session_store::{NewSession, SessionStore, StoreError};
use tokio::sync::{mpsc, oneshot, watch};

use crate::Engine;
use crate::run_lock::RunGuard;

const DUPLICATE_CALL: &str = "error: duplicate call. Use the earlier result.";
const TOOL_TIMED_OUT: &str = "error: tool timed out";
const MALFORMED_CALL_NAME: &str = "malformed";
const MALFORMED_TURN_LIMIT: u32 = 3;
const MALFORMED_TURNS_FAILED: &str = "the model did not produce a valid tool call";
const SUMMARY_MAX_TOKENS: u32 = 1024;
pub(crate) const REPLY_TOO_LONG: &str = "the model reply passed the size limit";
// A token is rarely longer than 16 characters, and a server can ignore max_tokens
const REPLY_CHARS_PER_TOKEN: usize = 16;
// Longest wait to deliver the closing frames of a cancelled run to a slow client
const CLOSING_FRAME_WAIT: Duration = Duration::from_secs(5);

pub struct ToolReply {
    pub output: String,
    pub is_error: bool,
}

struct PendingCall {
    call_id: String,
    reply: oneshot::Sender<ToolReply>,
}

// The single tool call that waits for the browser, because calls run one after another
#[derive(Clone, Default)]
pub struct ToolReplies {
    pending: Arc<Mutex<Option<PendingCall>>>,
}

impl ToolReplies {
    fn expect(&self, call_id: &str) -> oneshot::Receiver<ToolReply> {
        let (reply, receiver) = oneshot::channel();
        *self.slot() = Some(PendingCall {
            call_id: call_id.to_owned(),
            reply,
        });
        receiver
    }

    fn clear(&self) {
        self.slot().take();
    }

    pub fn resolve(&self, call_id: &str, reply: ToolReply) {
        let mut slot = self.slot();
        if slot
            .as_ref()
            .is_some_and(|pending| pending.call_id == call_id)
            && let Some(pending) = slot.take()
        {
            let _ = pending.reply.send(reply);
        }
    }

    fn slot(&self) -> std::sync::MutexGuard<'_, Option<PendingCall>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct RunLink {
    pub frames: mpsc::Sender<ServerFrame>,
    // Turns true on a cancel, and the sender drops when the connection closes
    pub cancelled: watch::Receiver<bool>,
    pub tool_replies: ToolReplies,
}

pub struct RunRequest {
    session: Session,
    text: String,
    attachments: Vec<Attachment>,
    workspace: WorkspaceSnapshot,
    capabilities: Vec<ToolName>,
    repo_rules: Option<String>,
}

pub fn agent_kind(origin: &SessionOrigin) -> AgentKind {
    match origin {
        SessionOrigin::Root | SessionOrigin::Fork { .. } => AgentKind::Primary,
        SessionOrigin::Delegate { .. } => AgentKind::DelegateChild,
    }
}

pub async fn prepare(
    engine: &Engine,
    session_id: &str,
    text: String,
    attachments: Vec<Attachment>,
    workspace: WorkspaceSnapshot,
    capabilities: Vec<ToolName>,
    repo_rules: Option<String>,
) -> Result<RunRequest, String> {
    if text.trim().is_empty() && attachments.is_empty() {
        return Err("the run needs a text or an attachment".to_owned());
    }
    let repo_rules = match repo_rules.as_deref() {
        None => None,
        Some(rules) => rules_text(rules).map_err(|_| {
            format!("repo rules must hold at most {RULES_CHARACTER_LIMIT} characters")
        })?,
    };
    let id = session_id.to_owned();
    let session = match engine.store.call(move |store| store.session(&id)).await {
        Ok(session) => session,
        Err(crate::store::StoreAccessError::Store(StoreError::SessionNotFound(_))) => {
            return Err("session not found".to_owned());
        }
        Err(error) => {
            tracing::error!(%error, "session lookup failed");
            return Err("storage error".to_owned());
        }
    };
    match agent_kind(&session.origin) {
        AgentKind::Primary => Ok(RunRequest {
            session,
            text,
            attachments,
            workspace,
            capabilities,
            repo_rules,
        }),
        AgentKind::DelegateChild => Err("a delegate child session takes no run".to_owned()),
    }
}

pub async fn execute(engine: Arc<Engine>, guard: RunGuard, link: RunLink, request: RunRequest) {
    let session_id = request.session.id.clone();
    tracing::info!(session = %session_id, "run started");
    let run = Run {
        engine,
        link,
        guard,
        capabilities: request.capabilities.clone(),
    };
    let report = match run.primary_user_event(&request).await {
        Ok(user) => run.agent(request.session, AgentKind::Primary, user).await,
        Err(halt) => run.finish(&session_id, Err(halt), String::new()).await,
    };
    tracing::info!(session = %session_id, outcome = outcome_name(&report.outcome), "run finished");
}

const fn outcome_name(outcome: &RunOutcome) -> &'static str {
    match outcome {
        RunOutcome::Complete => "complete",
        RunOutcome::Cancelled => "cancelled",
        RunOutcome::StepLimit => "step_limit",
        RunOutcome::Failed { .. } => "failed",
    }
}

enum Halt {
    Cancelled,
    Failed(String),
}

enum Finish {
    Complete,
    StepLimit,
    Failed(String),
}

struct AgentReport {
    outcome: RunOutcome,
    last_text: String,
}

// State of one agent loop, a delegate child has its own
struct Agent {
    session_id: SessionId,
    kind: AgentKind,
    last_text: String,
    previous_call: Option<(String, Value)>,
    problem_turns: u32,
}

struct ToolOutput {
    text: String,
    is_error: bool,
}

impl ToolOutput {
    fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
        }
    }
}

struct Reply {
    text: String,
    reasoning: String,
    native_calls: Vec<NativeToolCall>,
    usage: Option<Usage>,
}

// Characters left of one model reply, visible and reasoning text together
pub(crate) struct ReplyBudget(usize);

impl ReplyBudget {
    pub(crate) fn new(profile: &ModelProfile) -> Self {
        Self(profile.reply_reserve as usize * REPLY_CHARS_PER_TOKEN)
    }

    pub(crate) fn admits(&mut self, delta: &str) -> bool {
        match self.0.checked_sub(delta.chars().count()) {
            Some(rest) => {
                self.0 = rest;
                true
            }
            None => false,
        }
    }

    fn charge(&mut self, delta: &str) -> Result<(), Halt> {
        if self.admits(delta) {
            return Ok(());
        }
        Err(Halt::Failed(REPLY_TOO_LONG.to_owned()))
    }
}

#[derive(Default)]
struct PartialReply {
    visible: String,
    reasoning: String,
}

#[derive(Deserialize)]
struct DelegateArguments {
    task: String,
    model: Option<String>,
}

struct Run {
    engine: Arc<Engine>,
    link: RunLink,
    guard: RunGuard,
    capabilities: Vec<ToolName>,
}

impl Run {
    async fn primary_user_event(&self, request: &RunRequest) -> Result<EventBody, Halt> {
        let events = self.events(&request.session.id).await?;
        let workspace = (latest_snapshot(&events) != Some(&request.workspace))
            .then(|| request.workspace.clone());
        let (system_rules, repo_rules) = initial_rules(
            &events,
            self.engine.settings.system_rules.clone(),
            request.repo_rules.clone(),
        );
        let limit = self.engine.settings.tool_output_limit;
        let attachments = request
            .attachments
            .iter()
            .map(|attachment| Attachment {
                content: cap_output(&attachment.content, limit),
                ..attachment.clone()
            })
            .collect();
        Ok(EventBody::User {
            text: request.text.clone(),
            attachments,
            workspace,
            system_rules,
            repo_rules,
        })
    }

    async fn agent(&self, session: Session, kind: AgentKind, user: EventBody) -> AgentReport {
        let mut agent = Agent {
            session_id: session.id.clone(),
            kind,
            last_text: String::new(),
            previous_call: None,
            problem_turns: 0,
        };
        let result = self.drive(&mut agent, session, user).await;
        self.finish(&agent.session_id, result, agent.last_text)
            .await
    }

    async fn finish(
        &self,
        session_id: &str,
        result: Result<Finish, Halt>,
        last_text: String,
    ) -> AgentReport {
        let outcome = match result {
            Ok(Finish::Complete) => RunOutcome::Complete,
            Ok(Finish::StepLimit) => RunOutcome::StepLimit,
            Ok(Finish::Failed(message)) | Err(Halt::Failed(message)) => {
                RunOutcome::Failed { message }
            }
            Err(Halt::Cancelled) => RunOutcome::Cancelled,
        };
        self.deliver(ServerFrame::RunFinished {
            session_id: session_id.to_owned(),
            outcome: outcome.clone(),
        })
        .await;
        AgentReport { outcome, last_text }
    }

    async fn drive(
        &self,
        agent: &mut Agent,
        session: Session,
        user: EventBody,
    ) -> Result<Finish, Halt> {
        let id = agent.session_id.clone();
        self.send(ServerFrame::RunStarted {
            session_id: id.clone(),
        })
        .await?;
        self.record(&id, user).await?;
        let titled = self.session(&id).await?;
        if titled.title != session.title {
            self.send(ServerFrame::SessionChanged { session: titled })
                .await?;
        }
        for _ in 0..self.engine.settings.step_limit {
            if let Some(finish) = self.step(agent).await? {
                return Ok(finish);
            }
        }
        Ok(Finish::StepLimit)
    }

    async fn step(&self, agent: &mut Agent) -> Result<Option<Finish>, Halt> {
        let session = self.session(&agent.session_id).await?;
        let profile = self.engine.catalog.profile(&session.model);
        let offered = offered_tools(session.mode, &self.capabilities, agent.kind);
        let delegate_targets = if offered.contains(&ToolName::Delegate) {
            self.engine
                .catalog
                .delegate_targets(&self.engine.inference, &session.model)
                .await
        } else {
            Vec::new()
        };
        let scale = self.engine.token_scale(&profile.id);
        let mut events = self.events(&session.id).await?;
        for _ in 0..2 {
            let input = PromptInput {
                profile: &profile,
                mode: session.mode,
                capabilities: &self.capabilities,
                agent_kind: agent.kind,
                events: &events,
                delegate_targets: &delegate_targets,
                token_scale: scale,
            };
            let body = match plan_compaction(&input) {
                CompactionPlan::None => break,
                CompactionPlan::Elide { through } => EventBody::Compaction {
                    through,
                    kind: CompactionKind::Elide,
                },
                CompactionPlan::Summarize { through, messages } => EventBody::Compaction {
                    through,
                    kind: CompactionKind::Summary {
                        text: self.summarize(&profile, &messages).await?,
                        model: profile.id.clone(),
                    },
                },
            };
            events.push(self.record(&session.id, body).await?);
        }
        let rendered = render(&PromptInput {
            profile: &profile,
            mode: session.mode,
            capabilities: &self.capabilities,
            agent_kind: agent.kind,
            events: &events,
            delegate_targets: &delegate_targets,
            token_scale: scale,
        })
        .map_err(|error| Halt::Failed(error.to_string()))?;
        self.send(ServerFrame::ContextReport {
            session_id: session.id.clone(),
            model: profile.id.clone(),
            report: rendered.report.clone(),
        })
        .await?;
        let request =
            chat_request(&profile, &rendered).map_err(|error| Halt::Failed(error.to_string()))?;
        let reply = self.reply(&session.id, &profile, request).await?;
        let prompt_tokens = reply.usage.as_ref().map_or(0, |usage| usage.prompt_tokens);
        self.engine.set_token_scale(
            &profile.id,
            calibrate_token_scale(scale, rendered.unscaled_tokens, prompt_tokens),
        );

        let next_seq = events.last().map_or(1, |event| event.seq + 1);
        let call_id_prefix = format!("call_{next_seq}_");
        let native_calls = chat_tool_calls(reply.native_calls, &call_id_prefix);
        let parsed = parse_reply(
            profile.tool_codec,
            &reply.text,
            &native_calls,
            &call_id_prefix,
        );
        let failed_calls = problem_calls(
            parsed.problems,
            &native_calls,
            &parsed.tool_calls,
            &call_id_prefix,
        );
        let recorded_calls = parsed
            .tool_calls
            .iter()
            .chain(failed_calls.iter().map(|(call, _)| call))
            .cloned()
            .collect();
        self.record(
            &session.id,
            EventBody::Assistant {
                text: parsed.text.clone(),
                reasoning: reply.reasoning,
                tool_calls: recorded_calls,
                model: profile.id.clone(),
                usage: reply.usage,
            },
        )
        .await?;
        agent.last_text = parsed.text;
        for (call, problem) in &failed_calls {
            self.record_result(&session.id, call, ToolOutput::error(problem.as_str()))
                .await?;
        }

        if parsed.tool_calls.is_empty() {
            if failed_calls.is_empty() {
                return Ok(Some(Finish::Complete));
            }
            agent.problem_turns += 1;
            if agent.problem_turns >= MALFORMED_TURN_LIMIT {
                return Ok(Some(Finish::Failed(MALFORMED_TURNS_FAILED.to_owned())));
            }
            return Ok(None);
        }
        agent.problem_turns = 0;
        for call in &parsed.tool_calls {
            let output = self.execute_call(agent, &session, &offered, call).await?;
            self.record_result(&session.id, call, output).await?;
        }
        Ok(None)
    }

    async fn execute_call(
        &self,
        agent: &mut Agent,
        session: &Session,
        offered: &[ToolName],
        call: &ToolCall,
    ) -> Result<ToolOutput, Halt> {
        let tool = match validate_tool_call(call, offered) {
            Ok(tool) => tool,
            Err(error) => return Ok(ToolOutput::error(error.to_string())),
        };
        let current = (call.name.clone(), call.arguments.clone());
        if agent.previous_call.replace(current.clone()) == Some(current) {
            return Ok(ToolOutput::error(DUPLICATE_CALL));
        }
        match tool {
            ToolName::Read | ToolName::Glob | ToolName::Grep | ToolName::Edit | ToolName::Write => {
                self.workspace_tool(&session.id, call).await
            }
            ToolName::Delegate => Box::pin(self.delegate(session, call)).await,
        }
    }

    async fn workspace_tool(&self, session_id: &str, call: &ToolCall) -> Result<ToolOutput, Halt> {
        let reply = self.link.tool_replies.expect(&call.id);
        self.send(ServerFrame::ToolCall {
            session_id: session_id.to_owned(),
            call: call.clone(),
        })
        .await?;
        let waited = tokio::select! {
            biased;
            () = self.cancelled() => Err(Halt::Cancelled),
            reply = tokio::time::timeout(self.engine.settings.tool_timeout, reply) => Ok(reply),
        };
        self.link.tool_replies.clear();
        match waited? {
            Ok(Ok(reply)) => Ok(ToolOutput {
                text: reply.output,
                is_error: reply.is_error,
            }),
            Ok(Err(_)) => Err(Halt::Cancelled),
            Err(_) => Ok(ToolOutput::error(TOOL_TIMED_OUT)),
        }
    }

    async fn delegate(&self, parent: &Session, call: &ToolCall) -> Result<ToolOutput, Halt> {
        let arguments: DelegateArguments = match serde_json::from_value(call.arguments.clone()) {
            Ok(arguments) => arguments,
            Err(error) => {
                return Ok(ToolOutput::error(format!(
                    "error: invalid arguments for delegate: {error}"
                )));
            }
        };
        let catalog = &self.engine.catalog;
        let inference = &self.engine.inference;
        let model = match arguments.model {
            None => parent.model.clone(),
            Some(id) => match catalog.delegate_target(inference, &id).await {
                Some(profile) => profile.id,
                None => {
                    let choices: Vec<String> = catalog
                        .delegate_targets(inference, &parent.model)
                        .await
                        .into_iter()
                        .map(|profile| profile.id)
                        .collect();
                    return Ok(ToolOutput::error(format!(
                        "error: unknown model: {id}. Choose one of: {}",
                        choices.join(", ")
                    )));
                }
            },
        };
        let parent_events = self.events(&parent.id).await?;
        let (system_rules, repo_rules) = stored_rules(&parent_events);
        let new = NewSession {
            workspace: parent.workspace.clone(),
            model,
            mode: parent.mode,
            origin: SessionOrigin::Delegate {
                parent: parent.id.clone(),
                call_id: call.id.clone(),
            },
        };
        let child = self.store(move |store| store.create_session(new)).await?;
        self.send(ServerFrame::SessionChanged {
            session: child.clone(),
        })
        .await?;
        let user = EventBody::User {
            text: arguments.task,
            attachments: Vec::new(),
            workspace: latest_snapshot(&parent_events).cloned(),
            system_rules,
            repo_rules,
        };
        self.guard.set_child(Some(child.id.clone()));
        let report = self.agent(child, AgentKind::DelegateChild, user).await;
        self.guard.set_child(None);
        let reason = match report.outcome {
            RunOutcome::Complete => {
                return Ok(ToolOutput {
                    text: report.last_text,
                    is_error: false,
                });
            }
            RunOutcome::Cancelled => return Err(Halt::Cancelled),
            RunOutcome::StepLimit => "the delegate reached the step limit".to_owned(),
            RunOutcome::Failed { message } => format!("the delegate failed: {message}"),
        };
        if report.last_text.is_empty() {
            return Ok(ToolOutput::error(format!("error: {reason}")));
        }
        Ok(ToolOutput::error(format!(
            "error: {reason}\nLast answer of the delegate:\n{}",
            report.last_text
        )))
    }

    async fn summarize(
        &self,
        profile: &ModelProfile,
        messages: &[ChatMessage],
    ) -> Result<String, Halt> {
        let request = ChatRequest {
            model: profile.id.clone(),
            messages: wire_value(messages)?,
            tools: None,
            max_tokens: profile.reply_reserve.min(SUMMARY_MAX_TOKENS),
            temperature: profile.temperature,
            top_p: profile.top_p,
        };
        let mut budget = ReplyBudget::new(profile);
        let mut stream = pin!(self.engine.inference.chat(request));
        let mut text = String::new();
        loop {
            let item = tokio::select! {
                biased;
                () = self.cancelled() => return Err(Halt::Cancelled),
                item = stream.next() => item,
            };
            match item {
                Some(Ok(ChatChunk::Text(delta))) => {
                    budget.charge(&delta)?;
                    text.push_str(&delta);
                }
                Some(Ok(ChatChunk::Reasoning(delta))) => budget.charge(&delta)?,
                Some(Ok(ChatChunk::Finished(_))) => break,
                Some(Err(error)) => {
                    return Err(Halt::Failed(format!("the summary request failed: {error}")));
                }
                None => return Err(Halt::Failed("the summary stream ended early".to_owned())),
            }
        }
        let text = text.trim();
        if text.is_empty() {
            return Err(Halt::Failed("the model wrote an empty summary".to_owned()));
        }
        Ok(text.to_owned())
    }

    // On cancel, keeps the visible text that the client already shows
    async fn reply(
        &self,
        session_id: &str,
        profile: &ModelProfile,
        request: ChatRequest,
    ) -> Result<Reply, Halt> {
        let mut partial = PartialReply::default();
        let result = self
            .stream_reply(session_id, profile, request, &mut partial)
            .await;
        if !matches!(result, Err(Halt::Cancelled)) || partial.visible.trim().is_empty() {
            return result;
        }
        let body = EventBody::Assistant {
            text: partial.visible,
            reasoning: partial.reasoning,
            tool_calls: Vec::new(),
            model: profile.id.clone(),
            usage: None,
        };
        let id = session_id.to_owned();
        match self.store(move |store| store.append_event(&id, body)).await {
            Ok(event) => {
                self.deliver(ServerFrame::Event {
                    session_id: session_id.to_owned(),
                    event,
                })
                .await;
            }
            Err(Halt::Failed(message)) => tracing::error!(message, "partial reply not stored"),
            Err(Halt::Cancelled) => {}
        }
        Err(Halt::Cancelled)
    }

    async fn stream_reply(
        &self,
        session_id: &str,
        profile: &ModelProfile,
        request: ChatRequest,
        partial: &mut PartialReply,
    ) -> Result<Reply, Halt> {
        let mut stream = pin!(self.engine.inference.chat(request));
        let mut budget = ReplyBudget::new(profile);
        let mut splitter = StreamSplitter::new(profile.tool_codec);
        let mut native_reasoning = String::new();
        loop {
            let item = tokio::select! {
                biased;
                () = self.cancelled() => return Err(Halt::Cancelled),
                item = stream.next() => item,
            };
            let segments = match item {
                Some(Ok(ChatChunk::Text(delta))) => {
                    budget.charge(&delta)?;
                    splitter.push(&delta)
                }
                Some(Ok(ChatChunk::Reasoning(delta))) => {
                    budget.charge(&delta)?;
                    native_reasoning.push_str(&delta);
                    vec![prompt::StreamSegment {
                        channel: DeltaChannel::Reasoning,
                        text: delta,
                    }]
                }
                Some(Ok(ChatChunk::Finished(completion))) => {
                    let split = splitter.finish();
                    return Ok(Reply {
                        text: split.text,
                        reasoning: native_reasoning + &split.reasoning,
                        native_calls: completion.tool_calls,
                        usage: completion.usage,
                    });
                }
                Some(Err(error)) => return Err(Halt::Failed(error.to_string())),
                None => {
                    return Err(Halt::Failed(
                        "the inference stream ended without a completion".to_owned(),
                    ));
                }
            };
            for segment in segments {
                match segment.channel {
                    DeltaChannel::Text => partial.visible.push_str(&segment.text),
                    DeltaChannel::Reasoning => partial.reasoning.push_str(&segment.text),
                }
                self.send(ServerFrame::Delta {
                    session_id: session_id.to_owned(),
                    channel: segment.channel,
                    text: segment.text,
                })
                .await?;
            }
        }
    }

    async fn record_result(
        &self,
        session_id: &str,
        call: &ToolCall,
        output: ToolOutput,
    ) -> Result<(), Halt> {
        let body = EventBody::ToolResult {
            call_id: call.id.clone(),
            name: call.name.clone(),
            output: cap_output(&output.text, self.engine.settings.tool_output_limit),
            is_error: output.is_error,
        };
        self.record(session_id, body).await?;
        Ok(())
    }

    async fn record(&self, session_id: &str, body: EventBody) -> Result<Event, Halt> {
        let id = session_id.to_owned();
        let event = self
            .store(move |store| store.append_event(&id, body))
            .await?;
        self.send(ServerFrame::Event {
            session_id: session_id.to_owned(),
            event: event.clone(),
        })
        .await?;
        Ok(event)
    }

    async fn session(&self, session_id: &str) -> Result<Session, Halt> {
        let id = session_id.to_owned();
        self.store(move |store| store.session(&id)).await
    }

    async fn events(&self, session_id: &str) -> Result<Vec<Event>, Halt> {
        let id = session_id.to_owned();
        self.store(move |store| store.events(&id)).await
    }

    async fn store<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut SessionStore) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, Halt> {
        self.engine.store.call(operation).await.map_err(|error| {
            tracing::error!(%error, "session store request failed during a run");
            Halt::Failed("storage error".to_owned())
        })
    }

    async fn cancelled(&self) {
        let mut cancelled = self.link.cancelled.clone();
        let _ = cancelled.wait_for(|cancelled| *cancelled).await;
    }

    async fn send(&self, frame: ServerFrame) -> Result<(), Halt> {
        tokio::select! {
            biased;
            () = self.cancelled() => Err(Halt::Cancelled),
            sent = self.link.frames.send(frame) => sent.map_err(|_| Halt::Cancelled),
        }
    }

    // Sends a closing frame also after a cancel
    async fn deliver(&self, frame: ServerFrame) {
        let _ = tokio::time::timeout(CLOSING_FRAME_WAIT, self.link.frames.send(frame)).await;
    }
}

fn initial_rules(
    events: &[Event],
    system_rules: Option<String>,
    repo_rules: Option<String>,
) -> (Option<String>, Option<String>) {
    if events
        .iter()
        .any(|event| matches!(event.body, EventBody::User { .. }))
    {
        (None, None)
    } else {
        (system_rules, repo_rules)
    }
}

fn stored_rules(events: &[Event]) -> (Option<String>, Option<String>) {
    for event in events {
        if let EventBody::User {
            system_rules,
            repo_rules,
            ..
        } = &event.body
        {
            return (system_rules.clone(), repo_rules.clone());
        }
    }
    (None, None)
}

fn latest_snapshot(events: &[Event]) -> Option<&WorkspaceSnapshot> {
    events.iter().rev().find_map(|event| match &event.body {
        EventBody::User {
            workspace: Some(snapshot),
            ..
        } => Some(snapshot),
        EventBody::User {
            workspace: None, ..
        }
        | EventBody::Assistant { .. }
        | EventBody::ToolResult { .. }
        | EventBody::Compaction { .. }
        | EventBody::ModelSwitch { .. } => None,
    })
}

pub(crate) fn chat_request(
    profile: &ModelProfile,
    rendered: &RenderedPrompt,
) -> Result<ChatRequest, serde_json::Error> {
    Ok(ChatRequest {
        model: profile.id.clone(),
        messages: serde_json::to_value(&rendered.messages)?,
        tools: rendered
            .tools
            .as_ref()
            .map(serde_json::to_value)
            .transpose()?,
        max_tokens: profile.reply_reserve,
        temperature: profile.temperature,
        top_p: profile.top_p,
    })
}

fn wire_value<T: serde::Serialize + ?Sized>(value: &T) -> Result<Value, Halt> {
    serde_json::to_value(value).map_err(|error| Halt::Failed(error.to_string()))
}

// A server can omit a call id, and the result of the call needs one to find its call
pub(crate) fn chat_tool_calls(
    calls: Vec<NativeToolCall>,
    call_id_prefix: &str,
) -> Vec<ChatToolCall> {
    calls
        .into_iter()
        .enumerate()
        .map(|(index, call)| ChatToolCall {
            id: if call.id.is_empty() {
                format!("{call_id_prefix}{index}")
            } else {
                call.id
            },
            name: call.name,
            arguments: call.arguments,
        })
        .collect()
}

// Gives each problem of parse_reply a recorded call, so its error result reaches the model
// A native call that did not parse keeps its id, and a text block that did not parse gets a call named malformed
pub(crate) fn problem_calls(
    problems: Vec<String>,
    native_calls: &[ChatToolCall],
    parsed: &[ToolCall],
    call_id_prefix: &str,
) -> Vec<(ToolCall, String)> {
    let mut unparsed = native_calls
        .iter()
        .filter(|native| parsed.iter().all(|call| call.id != native.id));
    problems
        .into_iter()
        .enumerate()
        .map(|(index, problem)| {
            let call = match unparsed.next() {
                Some(native) => ToolCall {
                    id: native.id.clone(),
                    name: native.name.clone(),
                    arguments: Value::Object(serde_json::Map::new()),
                },
                None => ToolCall {
                    id: format!("{call_id_prefix}{MALFORMED_CALL_NAME}_{index}"),
                    name: MALFORMED_CALL_NAME.to_owned(),
                    arguments: Value::Object(serde_json::Map::new()),
                },
            };
            (call, problem)
        })
        .collect()
}
