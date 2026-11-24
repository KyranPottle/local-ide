---
description: How to run local-ide, build a release archive, and install it in a Proxmox container.
status: active
last_verified: 2026-11-24
---

# Deployment

This page explains how to run `local-ide` on one machine.
It also explains the release archive and the Proxmox container installer.

## Local run

Copy the example configuration.

```sh
cp config/local-ide.example.toml config/local-ide.toml
```

Run the later commands from the repository root.
The example paths are relative to that directory.
Set `inference.base_url` to your llama-swap server.
Replace each `[[model]]` id with a model name that your server lists.
The example ids `coder` and `fast` are placeholders.
[The technical spec](TECHNICAL-SPEC.md#your-models) explains each profile field.
Set the environment variable named by `inference.api_key_env`.
The example variable name is `LLAMA_SWAP_KEY`.
The engine stops at startup when that variable is missing or empty.
Build the web client first.
The engine requires `index.html` in `assets_dir`.

```sh
pnpm --dir web install
pnpm --dir web build
cargo run -p engine --bin local-ide -- --config config/local-ide.toml
```

Start the dev server in a second terminal.

```sh
pnpm --dir web dev
```

Open `http://127.0.0.1:5173`.
The dev server proxies `/api` to `127.0.0.1:7878`.
`public_origin` in the example is `http://127.0.0.1:5173`.
Change `public_origin` when the browser uses another origin.
Change `allowed_peers` when the client connects from another address.

## Release build

The release build needs these tools on the build machine.

- The Rust target `x86_64-unknown-linux-musl`.
- `zig` and `cargo-zigbuild`.
- `pnpm`.

Add the Rust target with this command.

```sh
rustup target add x86_64-unknown-linux-musl
```

Build the archive from the repository root.

```sh
deploy/build_release.sh
```

The script writes `dist/local-ide-<version>-x86_64-linux.tar.gz`.
It also writes a `.sha256` file beside the archive.
The version comes from `[workspace.package]` in `Cargo.toml`.
The archive holds the static binary and the web build.
It also holds the example config, the systemd unit, and the host scripts.

## Install the binary

Copy the binary and the web build onto the host.
Write a config file for that host.
Put the API key in a file that only root can read.
Point the service at that file with `EnvironmentFile`.
The unit in `deploy/local-ide.service` is one example.
It runs the binary as the user `local-ide`.
It reads `/etc/local-ide/key.env` and `/etc/local-ide/config.toml`.
It sets a memory cap and several systemd sandbox options.
Review those paths before you install the unit.

The engine has no TLS and no user login.
Bind `listen` to an address that only your proxy can reach.
Set `allowed_peers` to that proxy.
Set `public_origin` to the exact origin of the page.
A page on localhost is a secure context.
A remote browser needs HTTPS from a reverse proxy.
The file picker requires that secure context.

## Proxmox container installer

`deploy/deploy_to_container.sh` creates one LXC container on a Proxmox host.
It installs the release inside that container.
A normal machine uses Local run or Install the binary.
The host scripts live in `deploy/host/`.
Set the site variables in the environment before you run the script.
`deploy/host/container_defaults.sh` names each variable.
The values in that file belong to one site.
Override each value for any other site.
The addresses below are documentation examples.
Pass `--apply` to change the host.
With no argument, the script prints its usage and stops.

```sh
deploy/build_release.sh
PROXMOX_SSH_HOST=build-host \
TAILNET_HOST=ide.example.ts.net \
INFERENCE_BASE_URL=http://192.0.2.10:8080/v1 \
INFERENCE_CT_ID=200 \
CT_ID=201 \
CT_ADDRESS=192.0.2.11 \
CT_GATEWAY=192.0.2.1 \
CT_BRIDGE=vmbr0 \
deploy/deploy_to_container.sh --apply
```

`deploy/deploy_to_container.sh --check` prints each action.
It opens no connection.

The installer runs four host steps.

1. `create_container.sh` creates the container when it is absent, then starts it.
2. `install_release.sh` installs the binary, the web build, the config, the key, and the unit.
3. `publish_tailnet.sh` adds a Tailscale HTTPS entry for the engine.
4. `check_deployment.sh` checks engine health, the model list, and that HTTPS entry.

The install derives the engine config from the example file.
It replaces `listen`, `public_origin`, `allowed_peers`, `data_dir`, `assets_dir`, and `inference.base_url`.
The model profiles stay in the example file.
An existing config stays in place on a later install.
The installer writes the new candidate beside it as `config.toml.new`.
Compare the two files and merge the changes by hand.

Each step writes one log line with a UTC timestamp.
The host scripts record each completed step.
A second run skips a recorded step and continues after the last one.

Run the container test from the repository root when you change a host script.

```sh
LOCAL_IDE_CONTAINER_TESTS=1 deploy/tests/host_scripts_test.sh
```

The container tests stay idle until that variable is `1`.

The test runs the host scripts against stub commands.
It opens no connection to a host.
