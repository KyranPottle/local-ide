---
description: The contract of local-ide, with the wire types, the HTTP API, the WebSocket frames, the workspace tools, the context rules, and the engine configuration.
status: active
last_verified: 2026-11-24
---

# Technical Spec

## Goal

`local-ide` is a lightweight browser-based IDE for small local LLM models.
The design keeps the prompt small for profiles with a budget of 12000 to 24000 tokens.
The engine owns the agent loop, the context budget, the session storage, and the single run.
The browser owns the workspace and executes the file tools that the engine requests.

`crates/protocol` implements each wire type in this file.
When this file and the crate disagree, the tests in `crates/protocol/tests/wire_format.rs` are authoritative.

## Stack

| Part | Selection |
|------|-----------|
| Rust | stable toolchain with `rustfmt` and `clippy`, edition 2024, resolver 3 |
| Engine | axum 0.8.9 (`ws`), tokio 1.54.1, tower-http 0.7.1, rusqlite 0.43.0 (`bundled`), toml 1.2.2, uuid 1.29.3 (`v7`) |
| Inference client | reqwest 0.13.5 with `json` and `stream`, and with no TLS feature |
| Contract | serde 1.0.229, serde_json 1.0.154, ts-rs 12.0.1 (`serde-json-impl`) |
| Web | pnpm 10.14.0, Vite 8.3.8, SolidJS 1.9.20, vite-plugin-solid 2.11.17, TypeScript 7.0.2, Vitest 5.1.1, Playwright 1.61.1, CodeMirror 6 |

The root `Cargo.toml` declares each Rust dependency version in `[workspace.dependencies]`.
A crate takes a dependency with `workspace = true`.
The engine talks plain HTTP to the inference server, so `reqwest` has no TLS feature.

## Wire Conventions

- All JSON uses snake_case field names.
- An enum that carries data uses an internally tagged representation. The tag name is given with each enum.
- A tag value is the snake_case form of the variant name, for example `run_start`.
- An enum with unit variants only serializes as a snake_case string.
- An `Option` field serializes as `null`. A reader accepts an absent field as `null`.
- Each integer is a JSON number and a TypeScript `number`. Counters are `u32`.
- A timestamp is Unix time in milliseconds, stored as `i64`.

```json
{ "type": "run_finished", "session_id": "0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b", "outcome": { "kind": "step_limit" } }
```

## Core Types

```text
SessionId = String            UUID v7, lowercase hyphenated

enum Mode { Agent, Ask }

enum ToolName { Read, Glob, Grep, Edit, Write, Delegate }

enum SessionOrigin (tag "kind") {
  Root,
  Fork { parent: SessionId, seq: u32 },             copy of the parent events with seq <= seq
  Delegate { parent: SessionId, call_id: String },  child run started by a delegate tool call
}

struct Session {
  id: SessionId, title: String, workspace: String, model: String,
  mode: Mode, origin: SessionOrigin, created_at: i64 ms, updated_at: i64 ms
}

struct ToolCall { id: String, name: String, arguments: serde_json::Value }
struct Usage { prompt_tokens: u32, completion_tokens: u32, tokens_per_second: Option<f32> }
struct LineRange { first: u32, last: u32 }          1-based, inclusive
struct Attachment { path: String, lines: Option<LineRange>, content: String }
struct WorkspaceSnapshot { name: String, entries: Vec<String>, active_file: Option<String> }

enum CompactionKind (tag "kind") { Elide, Summary { text: String, model: String } }

enum EventBody (tag "type") {
  User { text: String, attachments: Vec<Attachment>, workspace: Option<WorkspaceSnapshot>, system_rules: Option<String>, repo_rules: Option<String> },
  Assistant { text: String, reasoning: String, tool_calls: Vec<ToolCall>, model: String, usage: Option<Usage> },
  ToolResult { call_id: String, name: String, output: String, is_error: bool },
  Compaction { through: u32, kind: CompactionKind },
  ModelSwitch { from: String, to: String },
}

struct Event { seq: u32, at: i64 ms, body: EventBody }
```

- `Event.seq` starts at 1 and increases by 1 inside one session.
- `ToolCall.name` stays a `String`, because a model can emit an unknown name.
- `Usage.tokens_per_second` is the generation speed that the inference server reports.

### Workspace Snapshot

- `WorkspaceSnapshot.entries` holds at most 60 top-level names. A directory name ends with `/`.
- The client sends the snapshot in each `RunStart`.
- The engine stores the snapshot on the `User` event when it differs from the latest stored snapshot.
- In each other case the engine stores `None`.
- A `Delegate` child gets the latest snapshot of the parent on its first `User` event.

### Rules

The engine stores `system_rules` and `repo_rules` on the first `User` event of a session.
`system_rules` comes from the configuration key `system_rules`.
`repo_rules` comes from `RunStart`.
A later `User` event stores null for both fields.
A change to either text applies to the next new session.
A fork keeps the texts in the copied events.
A `Delegate` child copies both texts from the first `User` event of the parent.

- Each text holds at most 2000 characters.
- `RULES_CHARACTER_LIMIT` in the protocol crate is that cap.
- The engine trims each text before the cap check.
- Blank text is stored as null.
- The browser reads `AGENTS.md` at the workspace root.
- A missing file sends null.
- A longer repo text rejects the run.
- A longer system text stops the engine at start.
- The rules follow the fixed prompt and the tool block.
- The headings are `System rules:` and `Repo rules:`.
- Empty rules add no characters.
- The prompt reads the first `User` event, including after compaction.
- A stored event can omit the two fields.
- The reader treats an absent field as null.

### Tools and Modes

- The browser executes the workspace tools `Read`, `Glob`, `Grep`, `Edit`, and `Write`.
- The engine executes `Delegate`.
- `Mode::tools()` gives all six tools in agent mode and `Read`, `Glob`, `Grep` in ask mode.
- The engine offers each workspace tool of the mode that `capabilities` lists.
- The engine offers `Delegate` in agent mode to a primary agent only, never to a `Delegate` child.

### Compaction

A `Compaction` event changes the model view only.
The store keeps each event.

- `Elide`: each `ToolResult` with `seq <= through` renders as a one-line stub.
- The stub is `[elided result of <name> <arguments> (N characters). Call the tool again if you need it.]`.
- A result that is not longer than its stub stays verbatim.
- `Summary`: the model sees the summary text in place of the events with `seq <= through`.
- The latest `Summary` wins. An `Elide` applies only to events after the latest `Summary`.

## Model Profile

```text
enum ToolCodec { Native, Text }

struct ModelProfile {
  id: String,                 exact llama-swap model name
  label: String,              display name
  role: String,               one line on the strength of the model
  context_budget: u32,        prompt token ceiling that the engine enforces, not the server n_ctx
  reply_reserve: u32,         sent as max_tokens
  tool_codec: ToolCodec,
  temperature: Option<f32>,
  top_p: Option<f32>,
  delegate_target: bool,      a delegating model may select this model
  chars_per_token: f32,       estimator seed, default 3.5
}

struct ModelInfo { profile: ModelProfile, available: bool }
```

- `GET /api/models` lists each configured profile, then each unconfigured model that the server lists.
- `available` is true when the inference server lists the model.
- `ModelProfile::unprofiled(id)` gives the profile of a model that the configuration does not list.
- Its values are `label = id`, `role = ""`, `context_budget = 16384`, `reply_reserve = 2048`, and `tool_codec = text`.
- It also has no sampling overrides, `delegate_target = false`, and `chars_per_token = 3.5`.

### Tool Codecs

`Native` uses the OpenAI `tools` request field and the `tool_calls` response field.
A native tool result returns as a `tool` role message.

`Text` puts the tool call in the model text, and the engine parses this block:

```text
<tool_call>
{"name": "read", "arguments": {"path": "src/main.rs"}}
</tool_call>
```

The parser in `crates/prompt/src/reply.rs` also accepts these forms:

- A missing `</tool_call>` at the end of the text or before the next block.
- A code fence around the JSON, with or without a language name.
- The key `parameters` or `args` in place of `arguments`.
- An `arguments` value that is a JSON-encoded string.
- No `arguments` key. The arguments are then an empty object.
- A whole reply that is one JSON object with a `name` and an arguments key, without tags.
- The argument tag form `read<arg_key>path</arg_key><arg_value>README.md</arg_value>` inside the tags.
- A tag value is text. It becomes an integer or a boolean only when the tool declaration gives that type.
- The unkeyed name form `{"read", "arguments": {...}}` inside the tags.

The stream splitter hides text from the first `<tool_call>` tag through the end of the reply.
A reply that is only a JSON object stays on the text channel until the parser reads it.
The engine writes each parsed call back into the history in the JSON form only.

A text tool result returns as a user role message:

```text
<tool_response>
...
</tool_response>
```

## HTTP API

The API uses JSON and the prefix `/api`.

```text
GET    /api/health                 200 text "ok"
GET    /api/status                 EngineStatus { version: String, active_run: Option<SessionId> }
GET    /api/models                 Vec<ModelInfo>
GET    /api/sessions?q=<text>      Vec<Session>, newest updated_at first
POST   /api/sessions               body CreateSession { workspace: String, model: String, mode: Mode } -> Session
GET    /api/sessions/{id}          SessionDetail { session: Session, events: Vec<Event>, context: ContextReport }
PATCH  /api/sessions/{id}          body UpdateSession { title: Option<String>, model: Option<String>, mode: Option<Mode> } -> Session
DELETE /api/sessions/{id}          204
POST   /api/sessions/{id}/fork     body ForkSession { seq: u32 } -> Session
```

- `q` is optional. It is a full-text prefix search over the titles and the event text.
- `q`, `title`, `workspace`, and `model` hold at most 200 characters.
- `workspace` and `model` must not be blank.
- A request body holds at most 1 MiB.
- `SessionDetail.context` is the estimate for the session model.
- The estimate includes each workspace tool that the mode allows.
- A primary session in agent mode also includes `Delegate`.
- In `UpdateSession`, an absent field and a `null` field both keep the stored value.
- A model change through `PATCH` appends a `ModelSwitch` event.
- `DELETE` also deletes the `Delegate` children of the session.
- A fork copies the events through `seq` into a new session with origin `Fork`.
- A fork during an active run is permitted.
- The title defaults to the first line of the first user text, cut at 60 characters.
- An error is JSON `ApiError { message: String }`.

| Status | Meaning |
|--------|---------|
| 400 | Invalid request. |
| 403 | Peer outside `allowed_peers`, or a missing or wrong `Origin`. |
| 404 | Unknown session or unknown route. |
| 409 | `PATCH` or `DELETE` of a session that takes part in the active run. |
| 500 | Storage error. |

The engine checks `Origin` on each request other than `GET`, `HEAD`, and `OPTIONS`, and on the WebSocket upgrade.

## WebSocket

The endpoint is `/api/ws`.
Each frame is a JSON text frame of at most 8 MiB.

```text
enum ClientFrame (tag "type") {
  RunStart { session_id: SessionId, text: String, attachments: Vec<Attachment>, workspace: WorkspaceSnapshot, capabilities: Vec<ToolName>, repo_rules: Option<String> },
  RunCancel,
  ToolResult { call_id: String, output: String, is_error: bool },
}

enum DeltaChannel { Text, Reasoning }
struct ContextReport { budget: u32, reserve: u32, system: u32, tools: u32, history: u32, total: u32 }
enum RunOutcome (tag "kind") { Complete, Cancelled, StepLimit, Failed { message: String } }
enum RejectReason (tag "kind") { Busy { session_id: SessionId }, InvalidRequest { message: String } }

enum ServerFrame (tag "type") {
  RunStarted { session_id: SessionId },
  Delta { session_id: SessionId, channel: DeltaChannel, text: String },
  ToolCall { session_id: SessionId, call: ToolCall },
  Event { session_id: SessionId, event: Event },
  ContextReport { session_id: SessionId, model: String, report: ContextReport },
  SessionChanged { session: Session },
  RunFinished { session_id: SessionId, outcome: RunOutcome },
  Rejected { reason: RejectReason },
}
```

- `ContextReport` holds estimated tokens. `total` is the sum of `system`, `tools`, and `history`.
- The engine sends `ContextReport` before each model request.
- `ServerFrame::Event` carries an event that the engine persisted. It is authoritative over the deltas.
- `ServerFrame::SessionChanged` reports a title change or a new `Delegate` child session.
- `RejectReason::Busy.session_id` is the session that has the active run.

Rules:

- One run is active in the whole engine at one time.
- A `RunStart` during an active run gets `Rejected` with `Busy`.
- A `RunStart` with no text and no attachment gets `Rejected` with `InvalidRequest`.
- A `RunStart` for an unknown session or a `Delegate` child session gets `Rejected` with `InvalidRequest`.
- A `RunStart` with repo rules over the cap gets `Rejected` with `InvalidRequest`.
- An unparsable or binary frame gets `Rejected` with `InvalidRequest`.
- A run belongs to the WebSocket connection that started it.
- When that connection closes, the engine cancels the run.
- Frames of a `Delegate` child run carry the child session id.
- The engine executes the tool calls of one assistant turn one after another, in order.
- A browser that does not answer a tool call within `tool_timeout_seconds` gets `error: tool timed out`.
- On cancel, the engine stores the partial visible text as an `Assistant` event with no tool calls.
- It stores nothing when the visible text is blank.

## Workspace Tool Contract

The browser executes the workspace tools in `web/src/agent/`.
The engine writes the descriptions that the model sees.
A path is a workspace-relative POSIX path.
A leading `/` or a `..` segment gives `error: path outside workspace`.

```text
read   { path: string, offset?: integer >= 1 (default 1), limit?: integer >= 1 }
       Output: one line per file line as `{line_number}|{text}`, from offset through the end of the file.
       A limit stops after that many lines.
       When lines remain, a last line `[lines A-B of N. Call read with offset=B+1 for more.]`.
       Errors: `error: file not found: <path>`, `error: not a file: <path>`, `error: binary file: <path>`,
       `error: offset N is past the end of <path> (M lines)`.

glob   { pattern: string }
       Output: matching file paths, sorted, one per line, at most 100,
       then `[N more paths not shown. Narrow the pattern.]`.
       No match: `no matches`.

grep   { pattern: string (JavaScript regular expression), path?: string (file or directory), glob?: string }
       Output: `{path}:{line_number}|{text}` per match, text cut at 200 characters, at most 50 matches,
       then `[more matches not shown. Narrow the pattern or the path.]`.
       Skips binary files.
       No match: `no matches`.

edit   { path: string, old: string, new: string, all?: boolean (default false) }
       Output: `edited <path>: N replacement(s)`.
       Errors: `error: old text not found in <path>`,
       `error: old text matches N places in <path>. Add surrounding lines or set all=true.`,
       `error: old text is empty`, `error: old and new are the same`.

write  { path: string, content: string }
       Output: `wrote <path> (N lines)`.

delegate { task: string, model?: string }     engine tool
       Runs one child agent in a new session with a fresh context.
       Returns the final answer of the child.
```

- `glob` and `grep` skip `.git`, `node_modules`, `target`, `dist`, `build`, `.next`, `.venv`, and `__pycache__`.
- `glob` and `grep` skip paths that hold a character below U+0020 or U+007F.
- Unsaved editor text gives `error: <path> has unsaved changes in the editor. Ask the user to save or discard them.`
- A file that changed during approval gives `error: <path> changed on disk during the approval. Call the tool again.`
- A declined change gives `error: the user declined this change` with `is_error = true`.
- No open workspace gives `error: no workspace is open`.
- `delegate` without `model` uses the model of the parent.
- `delegate` with an unknown model gives `error: unknown model: <id>. Choose one of: <ids>`.
- A failed child gives an error result with the reason and the last answer of the child.

The change journal keeps the last 200 edits and writes with the earlier and the new file text.
The total recorded character count stays at or below 16000000.
The client drops the oldest entries until the total fits.
One entry that alone passes the limit is not recorded.
`revert` for an unrecorded call id answers `unknown_change`.

## Context Rules

The `prompt` crate and `crates/engine/src/run.rs` implement these rules.

- The base system prompt holds at most 1400 characters in each mode and agent kind.
- The rules suffix follows the tool block.
- The system token count includes that suffix.
- The suffix adds no message overhead.
- The text tool block with all six tools holds at most 1200 characters.
- The native `tools` JSON with all six tools holds at most 2400 characters.
- Each tool description is one line.
- The only automatic file text in the prompt is `AGENTS.md`, as repo rules.
- A `User` event with a snapshot renders a `<workspace>` block before the user text.
- The block holds one entry per line, so a model does not read `src/ README.md` as one path.
- The block holds the workspace name, the top-level entries, and the active file path.
- The engine caps each attachment content at `tool_output_limit`.
- Rendering is deterministic and append-only between compactions.
- Reasoning text never returns to the model.

Token estimate:

- Each message counts its characters plus 16 characters of overhead.
- The estimate is the character count divided by `chars_per_token`, times a scale per model.
- After each reply, the scale becomes `usage.prompt_tokens` divided by the unscaled estimate.
- The scale stays between 0.5 and 2.0, and only in memory.

Compaction:

- The ceiling is `context_budget - reply_reserve`.
- No compaction happens with fewer than 8 turn events after the latest `Summary`.
- The newest 6 turn events stay verbatim.
- The engine writes an `Elide` when the elided prompt fits in 70 percent of the ceiling.
- Otherwise the active model writes a `Summary` with at most 1024 reply tokens.

Run limits:

- A run stops at `step_limit` model requests, with outcome `StepLimit`.
- One reply holds at most `reply_reserve * 16` characters of visible and reasoning text together.
- A longer reply fails the run with `the model reply passed the size limit`.
- The engine caps each tool output except a `read` at `tool_output_limit` characters.
- It then appends a line `[output truncated at N characters]`.
- A `read` result stays whole when its token estimate is less than `context_budget`.
- The estimate uses `chars_per_token` and the model scale.
- A larger read is stored as an error: `error: this read is N tokens and the context window is M tokens. Use offset and limit for a smaller range.`
- A call with the same name and arguments as the previous call of the agent gets `error: duplicate call. Use the earlier result.`.
- An unknown or unoffered tool name gets `error: unknown tool: <name>`, and the run continues.
- Invalid arguments get `error: invalid arguments for <name>: <reason>`.
- A tool call that does not parse gets an error result, under a call named `malformed` in the text codec.
- Three turns in a row with only failed calls end the run.
- The outcome message is `the model did not produce a valid tool call`.
- A model switch changes only the profile of the next request.

## Engine Configuration

The engine reads one TOML file from `--config <path>` or from `LOCAL_IDE_CONFIG`.
[The example configuration](../config/local-ide.example.toml) holds each key.
An unknown key fails the start.

| Key | Example | Meaning |
|-----|---------|---------|
| `listen` | `"127.0.0.1:7878"` | Socket address of the HTTP server. |
| `public_origin` | `"http://127.0.0.1:5173"` | The only `Origin` that the engine accepts. |
| `allowed_peers` | `["127.0.0.1"]` | IP addresses that may connect. At least one. |
| `data_dir` | `"./var"` | Directory that holds `sessions.sqlite`. |
| `assets_dir` | `"./web/dist"` | Web build that the engine serves at `/`. It must hold `index.html`. |
| `step_limit` | `25` | Maximum number of model requests in one run. At least 1. |
| `tool_output_limit` | `6000` | Character limit of one tool output other than a file read. At least 1. |
| `tool_timeout_seconds` | `300` | Longest wait for the browser to answer one tool call. |
| `system_rules` | absent | Instructions for every new session. At most 2000 characters. Blank means none. |
| `inference.base_url` | `"http://127.0.0.1:8080/v1"` | Base URL of the llama-swap server. Set this to your server. |
| `inference.api_key_env` | `"LLAMA_SWAP_KEY"` | Name of the environment variable that holds the API key. |
| `inference.idle_timeout_seconds` | `600` | Longest wait for the next bytes of a reply. |
| `[[model]]` | one table per profile | A `ModelProfile` with the fields of [Model Profile](#model-profile). |

- `public_origin` is `scheme://host[:port]` with `http` or `https` and no path.
- The start fails when the variable that `inference.api_key_env` names is unset or empty.
- A `[[model]]` table can omit `temperature`, `top_p`, and `chars_per_token`.
- Each `id` is unique and not blank.
- `reply_reserve` is at least 1 and less than `context_budget`.
- `crates/protocol/tests/example_config.rs` decodes each `[[model]]` table of the example as a `ModelProfile`.

### Your Models

Copy [the example configuration](../config/local-ide.example.toml) to `config/local-ide.toml`.
Replace each `id` with a model name that your inference server lists.
The `id` must match that name exactly.
Set `label` and `role` to the text you want in the model menu.
Set `context_budget` to the prompt ceiling you want the engine to enforce.
Set `reply_reserve` to the `max_tokens` value for one reply.
Set `tool_codec` to `native` or `text`.
Set `delegate_target` to `true` when another model may call this one.
Omit `chars_per_token` until you measure it. The default is `3.5`.
Run `local-ide calibrate` and copy the proposed `tool_codec` and `chars_per_token` into the file.
[Calibration](CALIBRATION.md) explains the report.

The example file holds two placeholder profiles, `coder` and `fast`.
Delete a placeholder that you do not use.
Add one `[[model]]` table for each model you keep.

## Calibration

`local-ide calibrate --config <path> [--model <id>]... [--output <path>] [--state <path>] [--fresh]` measures model profiles.
`crates/engine/src/calibration/` holds the command.
Each `--model` must name a configured profile. Without `--model`, the command selects each configured profile.
The command binds `listen` first and stops when the bind fails, because the service must not run.
It measures the selected models one after another, with one request at a time.

For each model that the inference server lists, the command sends these probes:

1. A short load request. It records the time to the first stream item and the reported tokens per second.
2. Two source code texts with `max_tokens = 1`. `chars_per_token` is the extra characters divided by the extra prompt tokens.
3. The real native prompt with the task `Read the file README.md.`, three times. A pass is a valid `read` of `README.md`.
4. The same task with the text codec, three times. A pass is exactly one valid `read` call and no problem line.
5. For each codec with at least one pass, an edit task in `src/answer.ts`, three times, with at most 6 requests each.

The edit task scores three stages: a read of the file, a correct edit, and a final text with no tool call.
The proposal is `native` when the native probe passes 3 of 3 and its task score is at least the text score.
Otherwise it is `text` when the text probe passes at least 2 of 3.
Otherwise the model is not fit for agent mode and keeps its codec.

The report on stdout has one block per model.
A failed tool call probe whose reply did not parse shows the raw reply text after the problem line.
`--output` writes the `[[model]]` tables of the measured models with the proposed `tool_codec` and `chars_per_token`.
The command never changes the configuration file.
The exit code is 0 when each selected model has a measurement, 2 when a model is missing or a probe could not run, and 1 on an error.
The state file defaults to `calibration-state.json` in `data_dir`, and the command saves it after each model.
A state file with another probe set version stops the command, unless `--fresh` replaces it.
The state file and the output file have mode 600.

## TypeScript Bindings

- `cargo test -p protocol` writes one file per type into `web/src/protocol/`. Run it from the repository root.
- `.cargo/config.toml` sets `TS_RS_EXPORT_DIR` to that directory.
- `crates/protocol/tests/typescript_index.rs` writes `web/src/protocol/index.ts`.
- `SessionId` has no binding of its own. Each `SessionId` field is a TypeScript `string`.
- `ToolCall.arguments` has the type `JsonValue` from `web/src/protocol/serde_json/JsonValue.ts`.
- `UpdateSession` fields are optional in TypeScript. Each other `Option` field is required and nullable.
- ts-rs writes its files with semicolons.

## Non-Goals

- No shell tool in the first version.
- No second run at the same time.
- The only automatic file text in the prompt is `AGENTS.md`, as repo rules.
- No LLM call for session titles.
- No TLS in the engine. A reverse proxy supplies HTTPS.
- No support for Safari or Firefox.

## Open Questions

None. The text codec accepts the argument tag form and the unkeyed name form, see [Tool Codecs](#tool-codecs).
