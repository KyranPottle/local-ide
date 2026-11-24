---
description: How to measure a model profile, read the report, and apply a proposal.
status: active
last_verified: 2026-11-24
---

# Calibration

`local-ide calibrate` measures each configured model against the inference server.
It writes a proposal.
It does not change the configuration file.
[The technical spec](TECHNICAL-SPEC.md#calibration) defines the probes, the codec rule, and the exit codes.

Stop the engine before you calibrate.
The command binds `listen`, and it stops when that bind fails.

```sh
local-ide calibrate --config config/local-ide.toml --output proposed-models.toml
```

Add `--model <id>` to measure one profile.
Repeat the flag to measure several profiles.
Add `--fresh` to measure each selected model again.
The state file keeps each finished model.
A later run skips a finished model until you pass `--fresh`.

## Example report

The block below is an example.
It is not a measurement from this repository.

```text
model example-small
  load seconds: 2.50
  tokens per second: 40.0
  characters per token: 3.50, profile 3.50
  native tool call: 3 of 3
  text tool call: 2 of 3
  native edit task: read 3 of 3, edit 3 of 3, finish 2 of 3
  text edit task: read 2 of 3, edit 2 of 3, finish 2 of 3
  proposed tool_codec: native
```

`load seconds` is the time to the first stream item.
`tokens per second` is the speed that the server reports.
`characters per token` is the measured estimator seed.
The second number is the value already stored on the profile.
A tool call line counts valid reads of `README.md`.
An edit line counts the read, the correct edit, and the final answer.
A parse failure can print the raw reply after the problem line.

Native wins when its probe passes 3 of 3.
Its task score must be at least the text score.
Text wins when its probe passes at least 2 of 3.
Otherwise the model is not fit for agent mode.
The profile then keeps its current codec.

## Proposal

`--output` writes one `[[model]]` table for each measured model.
The table carries the proposed `tool_codec` and `chars_per_token`.
Copy those two fields into your configuration when you accept them.
Restart the engine after you change the file.

The container installer wraps the same command.
Set `LOCAL_IDE_APPLY_CONTAINER` to `1` before you run either host script.
`deploy/host/calibrate_models.sh` stops the service, runs the command, and starts the service again.
`deploy/host/apply_calibration.sh` takes one proposal path.
It replaces each matching model table, restarts the service, and restores the backup when health fails.
A proposal that changes nothing makes no backup and no restart.
