---
description: The components of local-ide as built, the data flow of one run, each decision with its reason, the module boundaries, and the known limits.
status: active
last_verified: 2026-11-24
---

# Architecture

## Overview

`local-ide` has three parts: a Rust engine, a web client, and a llama-swap inference server.
The engine owns the agent loop, the context budget, the session storage, and the single run.
The web client owns the workspace and executes the workspace tools.
llama-swap keeps one model on the GPU.

## Components

| Path | Concern |
|------|---------|
| `crates/protocol` | Wire types of the HTTP API, the WebSocket, and the model profiles, with the TypeScript export. |
| `crates/session-store` | SQLite storage of sessions as append-only event logs, with full-text search. |
| `crates/prompt` | Pure rendering of an event log for one profile, with compaction, token estimates, and reply parsing. |
| `crates/inference` | Streaming client for the llama-swap chat completions endpoint. |
| `crates/engine` | The `local-ide` binary: HTTP API, WebSocket, static assets, access policy, run lock, and agent loop. |
| `web/src/agent` | Tool host for the workspace tools, change journal, search worker, and session state. |
| `web/src/agentPanel` | Chat transcript, composer, Markdown view, and diff rows. |
| `web/src/design` | Design tokens, fonts, icons, and the color scheme. |
| `web/src/editor` | CodeMirror 6 editor area with languages and theme. |
| `web/src/engine` | HTTP client and WebSocket connection to the engine. |
| `web/src/fileFinder` | Quick file finder with fuzzy match. |
| `web/src/fileTree` | File tree of the open workspace. |
| `web/src/layout` | IDE layout, resizable panels, keyboard shortcuts, and failure notices. |
| `web/src/preferences` | Preferences stored in the browser. |
| `web/src/protocol` | Generated TypeScript bindings of the wire types. |
| `web/src/sessionList` | Session list with relative times. |
| `web/src/statusBar` | Agent status in the status bar. |
| `web/src/toolbar` | Agent toolbar with the model picker and the context meter. |
| `web/src/welcome` | Welcome view to open a folder. |
| `web/src/workspace` | File System Access wrapper, recent workspaces, and the scratch workspace. |

[The technical spec](TECHNICAL-SPEC.md) holds the exact contract of each interface.

## Data Flow of One Run

1. The browser sends `RunStart` on the WebSocket at `/api/ws`.
2. The engine takes the run lock, or answers `Rejected` with `Busy`.
3. The engine validates the request, or answers `Rejected` with `InvalidRequest`.
4. The engine sends `RunStarted` and appends the `User` event.
5. The engine renders the event log and estimates the prompt tokens.
6. Over the ceiling, the engine appends at most two `Compaction` events and renders again.
7. The engine sends a `ContextReport` frame and the request to the inference server.
8. The engine forwards the reply stream as `Delta` frames.
9. The engine appends the `Assistant` event and sends it as an `Event` frame.
10. For each workspace tool call, in order, the engine sends a `ToolCall` frame.
11. The browser executes the tool and answers with a `ToolResult` frame.
12. The engine caps the output, appends the `ToolResult` event, and sends it as an `Event` frame.
13. For a `delegate` call, the engine runs a child agent in a new session and waits for its answer.
14. Steps 5 to 13 repeat until a reply has no tool call or the step limit stops the run.
15. The engine sends `RunFinished` and releases the run lock.

A cancel, a failure, or a closed WebSocket connection also ends the run.

## Decisions

1. **The engine is one Rust binary with axum, tokio, and SQLite.**
   Reason: low memory, a fast start, and one process to run.
2. **The agent loop runs in the engine. The workspace tools run in the browser through the File System Access API.**
   Reason: the files are on the client machine, and the server owns the GPU queue and the context budget.
3. **One run at a time, enforced by one run lock in the engine. Sequential `Delegate` child runs are permitted.**
   Reason: the inference server keeps one model loaded, and two jobs at once reload it.
4. **A session is an append-only event log that is independent of the model. The engine renders it for the active profile on each request.**
   Reason: a model switch and a fork need no conversion.
5. **Rendering is prefix-stable between compaction events.**
   Reason: llama.cpp reuses the prompt cache only for an unchanged prefix.
6. **Six tools with one-line descriptions, a small system prompt, and capped tool output.**
   Reason: a large system prompt and large tool descriptions overload small models.
7. **A tool-call codec per model profile, native or text.**
   Reason: some models emit native tool calls, and others emit the text format.
8. **Web UI: SolidJS, Vite, TypeScript, CodeMirror 6, CSS custom properties, self-hosted Inter and JetBrains Mono.**
   Reason: a small bundle, and fine-grained updates for token streams.
9. **The engine has no TLS. A reverse proxy supplies HTTPS.**
   Reason: the file picker needs a secure context, and localhost is already one.
10. **A Chromium-based browser is required, such as Chrome, Edge, or Brave.**
    Reason: Safari and Firefox have no writable directory picker.
    Brave turns off the File System Access API by default. The user sets `brave://flags/#file-system-access-api` to `Enabled` and relaunches Brave.
11. **No shell tool in the first version.**
    Reason: a browser cannot start processes.
12. **System rules come from the engine configuration. Repo rules come from `AGENTS.md` in the open folder.**
    Reason: the engine has no view of the folder. The browser reads that one file. Both texts freeze on the first user event, so the prompt prefix stays stable.

## Module Boundaries

- `protocol` depends on no other workspace crate.
- `session-store`, `prompt`, and `inference` depend on `protocol` only.
- `engine` depends on all four crates.
- `web` uses the generated bindings in `web/src/protocol/` and no Rust code.
- `prompt` stays pure: the same input gives the same output, with no I/O.
- Only `engine` holds the run lock, and only `engine` calls `inference`.
- The browser owns path safety. `prompt` validation lets an unsafe path pass to the browser.
- Model output is not trusted, so `ToolCall.name` stays a `String`.
- The engine accepts only peers in `allowed_peers` and checks `Origin` against `public_origin`.
- Each response sets `nosniff`, `no-referrer`, and a content security policy. The values live in `crates/engine/src/access.rs`.
- The inference key comes from the environment variable that `inference.api_key_env` names.

## Known Limits

- The UI needs a Chromium-based browser, because it uses the File System Access API.
- No shell tool exists, because a browser cannot start processes.
- The `capabilities` list in `RunStart` permits a later native tool host.
- The engine has no login.
- Access control is the peer list and the `Origin` check.
- A remote page needs HTTPS from a reverse proxy. Localhost is already a secure context.
- The example profiles match one model set. Calibrate a model before you trust it.
- A second page learns that the engine is free by a status poll every 5000 ms.
- `AGENTS.md` in an open folder is instructions for the model.
- A rules change applies to the next new session.
- A change to `system_rules` applies after an engine restart.
