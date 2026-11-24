# shellcheck shell=bash

# shellcheck source=serve_entry.sh
source "$(dirname "${BASH_SOURCE[0]}")/serve_entry.sh"

log() {
  printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >&2
}

fail() {
  log "error: $*"
  exit 1
}

require_container_apply() {
  [[ ${LOCAL_IDE_APPLY_CONTAINER:-} == 1 ]] || fail "set LOCAL_IDE_APPLY_CONTAINER=1 to run $(basename "$0")"
}

require_container_values() {
  local name
  for name in "${CONTAINER_VARIABLES[@]}"; do
    # Values reach command arguments, TOML strings, and remote shells
    [[ ${!name} =~ ^[A-Za-z0-9._:/@+-]*$ ]] || fail "$name has characters outside [A-Za-z0-9._:/@+-]"
  done
  for name in INFERENCE_CT_ID CT_ID CT_PREFIX CT_CORES CT_MEMORY_MIB CT_SWAP_MIB CT_DISK_GIB ENGINE_PORT SERVE_PORT; do
    [[ ${!name} =~ ^[0-9]+$ ]] || fail "$name is not a number"
  done
  [[ $KEY_VARIABLE =~ ^[A-Z_][A-Z0-9_]*$ ]] || fail "KEY_VARIABLE is not an environment variable name"
}

state_file() {
  printf '%s/state' "$DEPLOY_STATE_DIR"
}

state_init() {
  install -d -m 700 "$DEPLOY_STATE_DIR"
  [[ -f $(state_file) ]] || install -m 600 /dev/null "$(state_file)"
}

state_value() {
  sed -n "s/^$1 //p" "$(state_file)" | tail -n 1
}

state_set() {
  local file
  file=$(state_file)
  sed -i "/^$1 /d" "$file"
  printf '%s %s\n' "$1" "$2" >>"$file"
  log "state: $1 $2"
}

state_forget_steps() {
  sed -i "/^step $1\\./d" "$(state_file)"
}

absent() {
  return 1
}

run_step() {
  local name=$1 probe=$2 action=$3
  if grep -qxF "step $name" "$(state_file)"; then
    log "$name: recorded as complete, skipped"
    return
  fi
  if "$probe"; then
    log "$name: result present, skipped"
  else
    "$action"
    log "$name: done"
  fi
  printf 'step %s\n' "$name" >>"$(state_file)"
}

engine_healthy() {
  local attempt body
  for attempt in $(seq 1 15); do
    body=$(curl -fsS --max-time 5 "${ENGINE_URL}/api/health" 2>/dev/null || true)
    if [[ $body == ok ]]; then
      log "check: ${ENGINE_URL}/api/health answers ok"
      return 0
    fi
    log "check: health attempt $attempt of 15 failed"
    sleep 2
  done
  return 1
}

container_exists() {
  pct config "$1" >/dev/null 2>&1
}

address_in_use() {
  grep -qsE "ip=${CT_ADDRESS//./\\.}/" "$PVE_CONFIG_DIR"/lxc/*.conf "$PVE_CONFIG_DIR"/qemu-server/*.conf && return 0
  ping -c 1 -W 1 "$CT_ADDRESS" >/dev/null 2>&1
}

require_preconditions() {
  require_container_values
  command -v pct >/dev/null || fail "pct is absent: run this script on the Proxmox host"
  command -v tailscale >/dev/null || fail "tailscale is absent"
  command -v curl >/dev/null || fail "curl is absent"
  perl -MJSON::PP -e 1 2>/dev/null || fail "perl with JSON::PP is absent"
  serve_entry_present || true
  container_exists "$INFERENCE_CT_ID" || fail "container $INFERENCE_CT_ID (inference) is absent"
  if container_exists "$CT_ID"; then
    local hostname
    hostname=$(pct config "$CT_ID" | sed -n 's/^hostname: //p')
    [[ $hostname == "$CT_HOSTNAME" ]] || fail "id $CT_ID is in use by container '$hostname', not '$CT_HOSTNAME'"
    return
  fi
  if address_in_use; then
    fail "address $CT_ADDRESS is in use"
  fi
}

wait_for_guest() {
  local attempt state
  for attempt in $(seq 1 30); do
    state=$(pct exec "$CT_ID" -- systemctl is-system-running 2>/dev/null || true)
    case $state in
      running | degraded) return ;;
    esac
    log "guest $CT_ID state '${state:-unreachable}', attempt $attempt of 30"
    sleep 2
  done
  fail "guest $CT_ID did not finish boot"
}
