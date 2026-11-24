#!/usr/bin/env bash
set -euo pipefail

host_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=container_defaults.sh
source "${host_dir}/container_defaults.sh"
# shellcheck source=deploy_steps.sh
source "${host_dir}/deploy_steps.sh"

publish_entry() {
  tailscale serve --bg --https="$SERVE_PORT" "$ENGINE_URL" >&2
  serve_entry_status || fail "tailscale serve does not show the entry for port $SERVE_PORT"
}

require_container_apply
require_preconditions
state_init
run_step publish.serve serve_entry_present publish_entry
