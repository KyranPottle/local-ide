---
description: Agent rules for local-ide, with the entry point, the required checks, and the hard rules.
status: active
last_verified: 2026-11-24
---

# Agent Notes

Read [docs/README.md](docs/README.md) before another file in `docs/`.

## Checks Before a Change Is Done

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir web typecheck
pnpm --dir web test
pnpm --dir web e2e
```

The checks above leave the container installer and its tests idle.

## Hard Rules

- `cargo test -p protocol` writes the files in `web/src/protocol/`. Never edit them by hand.
- Change `crates/protocol` and [docs/TECHNICAL-SPEC.md](docs/TECHNICAL-SPEC.md) in the same patch.
- The `prompt` crate stays pure, with no I/O.
- One run at a time is an engine invariant.
- Rendering stays prefix-stable between compactions.
- Declare each Rust dependency version in the root `Cargo.toml` only.
- Use pnpm in `web/`, never npm. Write TypeScript without semicolons.
- Keep each secret out of the repository. The inference key comes from the environment.
