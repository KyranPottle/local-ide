#!/usr/bin/env bash
set -euo pipefail

host_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=container_defaults.sh
source "${host_dir}/container_defaults.sh"
# shellcheck source=deploy_steps.sh
source "${host_dir}/deploy_steps.sh"

check_models() {
  local available
  available=$(curl -fsS --max-time 30 "${ENGINE_URL}/api/models" |
    perl -MJSON::PP -e 'local $/; print scalar grep { $_->{available} } @{ decode_json(<STDIN>) }') ||
    fail "${ENGINE_URL}/api/models gives no model list"
  [[ $available -ge 1 ]] || fail "no model is available: check the key and the route to llama-swap"
  log "check: $available models available"
}

check_serve() {
  serve_entry_status || fail "tailscale serve shows no entry for port $SERVE_PORT to $ENGINE_URL"
  tailscale serve status | grep -q ":${SERVE_PORT}" || fail "tailscale serve status does not list port $SERVE_PORT"
  log "check: tailscale serve forwards port $SERVE_PORT to $ENGINE_URL"
}

require_container_apply
require_preconditions
engine_healthy || fail "${ENGINE_URL}/api/health does not answer ok"
check_models
check_serve
printf '%s\n' "$PUBLIC_URL"
