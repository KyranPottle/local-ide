---
description: Context map of local-ide, with the reading order and the condition to read each file.
status: active
last_verified: 2026-11-24
---

# local-ide Context

`local-ide` is a browser-based IDE for small local LLM models.
A Rust engine runs the agent loop and talks to a llama-swap inference server.
A Chromium-based browser opens a local folder and executes the file tools.

## Reading Order

1. [Architecture](ARCHITECTURE.md)
2. [Technical Spec](TECHNICAL-SPEC.md)
3. [Deployment](DEPLOYMENT.md), when you run the engine or build a release.
4. [Calibration](CALIBRATION.md), when you measure a model profile.

## Routes

- Read [Architecture](ARCHITECTURE.md) when you need the components, the data flow, the decisions, the boundaries, or the known limits.
- Read [Technical Spec](TECHNICAL-SPEC.md) when you change a wire type, a tool output, a context rule, or a configuration key.
- Read [Deployment](DEPLOYMENT.md) when you run the engine, build a release, or install it in a container.
- Read [Calibration](CALIBRATION.md) when you measure a model profile or read a calibration report.
- Read [the repository README](../README.md) when you need the build, test, and demo commands.
- Read [AGENTS.md](../AGENTS.md) when you need the hard rules and the checks before a change is done.
- Read [the wire tests](../crates/protocol/tests/wire_format.rs) when you need the literal JSON of a type.
- Read [the example configuration](../config/local-ide.example.toml) when you change an engine setting or a model profile.

## Do Not Read By Default

- `web/src/protocol/` holds generated TypeScript bindings. They repeat the `protocol` crate.
- `Cargo.lock` and `web/pnpm-lock.yaml` are lock files.

## Keeping This Current

Change the technical spec and the owning module in the same patch.
Move `last_verified` only after you compare each claim of a file against the code.
