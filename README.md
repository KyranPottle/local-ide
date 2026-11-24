---
description: What local-ide is, its requirements, and the commands to test, demo, build, and run it.
status: active
last_verified: 2026-11-24
---

# local-ide

`local-ide` is a lightweight browser IDE for local models running on consumer hardware.
A Rust engine runs the agent loop and sends each request to a llama-swap server.
The browser opens a local folder and runs the file tools that the engine requests.

## What it provides

The engine keeps one run at a time.
A session is an append-only event log.
The browser runs read, glob, grep, edit, and write.
The engine runs delegate in a child session.
Each model profile selects a native tool codec or a text tool codec.
The prompt stays small for a model with a short context budget.
The user sets system rules in the engine configuration.
The user sets repo rules in `AGENTS.md` at the workspace root.

## Requirements

- Rust stable with cargo, `rustfmt`, and `clippy`.
- Node and pnpm 10.14.0.
- Chrome, Edge, or Brave for the UI.
  Brave turns off the File System Access API by default.
  Set `brave://flags/#file-system-access-api` to Enabled.
  Relaunch Brave after that change.
- A llama-swap server, when you want a real model.
  The demo does not need one.
- For a release build, the target `x86_64-unknown-linux-musl`, `zig`, and `cargo-zigbuild`.

## Commands

Run each command from the repository root.

Install the web dependencies:

```sh
pnpm --dir web install
```

Demo with a rule-based responder in place of a model:

```sh
pnpm --dir web demo
```

Open the printed URL in Chrome, Edge, or Brave.

Run the engine against your inference server:

```sh
cp config/local-ide.example.toml config/local-ide.toml
pnpm --dir web build
```

Set `inference.base_url` in that copy to your llama-swap server.
Replace each `[[model]]` id with a model name that your server lists.
The example ids `coder` and `fast` are placeholders.
Set the environment variable named by `inference.api_key_env`.
The example name is `LLAMA_SWAP_KEY`.
[The technical spec](docs/TECHNICAL-SPEC.md#your-models) explains each profile field.
Start the engine:

```sh
cargo run -p engine --bin local-ide -- --config config/local-ide.toml
```

Start the dev server in a second terminal:

```sh
pnpm --dir web dev
```

Open `http://127.0.0.1:5173`.
The dev server proxies `/api` to the engine on port 7878.
`public_origin` in the example matches that page.

Checks:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir web typecheck
pnpm --dir web test
pnpm --dir web e2e
```

The checks above leave the container installer and its tests idle.

Release archive:

```sh
deploy/build_release.sh
```

[Deployment](docs/DEPLOYMENT.md) explains the local install, the archive, and the container installer.
[Calibration](docs/CALIBRATION.md) explains how to measure a model profile.
[The context map](docs/README.md) is the entry point for the other documents.
