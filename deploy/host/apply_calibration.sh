#!/usr/bin/env bash
set -euo pipefail

host_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=container_defaults.sh
source "${host_dir}/container_defaults.sh"
# shellcheck source=deploy_steps.sh
source "${host_dir}/deploy_steps.sh"

[[ $# -eq 1 ]] || fail "usage: apply_calibration.sh <proposal toml>"
proposal=$1
[[ -s $proposal ]] || fail "proposal $proposal is absent or empty"

guest_config="${GUEST_CONFIG_DIR}/config.toml"
guest_backup="${guest_config}.$(date -u +%Y%m%dT%H%M%SZ)"

guest() {
  pct exec "$CT_ID" -- "$@"
}

merge_model_tables() {
  awk '
    function table_id(text,    line, count, index_of) {
      count = split(text, line, "\n")
      for (index_of = 1; index_of <= count; index_of++) {
        if (line[index_of] ~ /^id[[:space:]]*=[[:space:]]*"[A-Za-z0-9._-]+"[[:space:]]*$/) {
          sub(/^id[[:space:]]*=[[:space:]]*"/, "", line[index_of])
          sub(/".*/, "", line[index_of])
          return line[index_of]
        }
      }
      return ""
    }
    function flush_proposal() {
      if (proposal_table == "") return
      id = table_id(proposal_table)
      if (id == "") { printf "apply_calibration: a proposal table has no valid id\n" > "/dev/stderr"; failed = 1 }
      replacement[id] = proposal_table
      proposal_table = ""
    }
    function flush_config() {
      if (config_table == "") return
      id = table_id(config_table)
      if (config_is_model && (id in replacement)) {
        trailing = config_table
        sub(/^[^\n]*(\n[^\n]+)*/, "", trailing)
        printf "%s%s", replacement[id], trailing
        applied[id] = 1
      } else {
        printf "%s", config_table
      }
      config_table = ""
    }
    FNR == 1 && NR != 1 { flush_proposal() }
    NR == FNR {
      if ($0 ~ /^[[:space:]]*$/ && proposal_table == "") next
      if ($0 ~ /^\[/) {
        if ($0 != "[[model]]") { printf "apply_calibration: the proposal holds %s\n", $0 > "/dev/stderr"; failed = 1 }
        flush_proposal()
      }
      if ($0 !~ /^[[:space:]]*$/) proposal_table = proposal_table (proposal_table == "" ? "" : "\n") $0
      next
    }
    /^\[/ { flush_config(); config_is_model = ($0 == "[[model]]") }
    { config_table = config_table $0 "\n" }
    END {
      flush_config()
      for (id in replacement) {
        if (!(id in applied)) { printf "apply_calibration: model %s is absent in the config\n", id > "/dev/stderr"; failed = 1 }
      }
      exit failed
    }
  ' "$1" "$2"
}

restart_healthy() {
  guest systemctl restart local-ide.service
  engine_healthy
}

require_container_apply
require_preconditions
container_exists "$CT_ID" || fail "container $CT_ID is absent: run create_container.sh first"
wait_for_guest
state_init

work=$(mktemp -d "${DEPLOY_STATE_DIR}/apply.XXXXXX")
trap 'rm -rf "$work"' EXIT
pct pull "$CT_ID" "$guest_config" "${work}/config.toml"
merge_model_tables "$proposal" "${work}/config.toml" >"${work}/config.next" ||
  fail "cannot merge $proposal into $guest_config"
if cmp -s "${work}/config.toml" "${work}/config.next"; then
  log "config: the proposal changes nothing, skipped"
  exit 0
fi

guest cp -p "$guest_config" "$guest_backup"
log "backup: $guest_backup"
gid=$(guest id -g "$GUEST_USER")
pct push "$CT_ID" "${work}/config.next" "${guest_config}.next" --perms 640 --user 0 --group "$gid"
guest mv -f "${guest_config}.next" "$guest_config"
log "config: model tables of $proposal applied"

if restart_healthy; then
  log "service: local-ide.service healthy with the new config"
  exit 0
fi
log "error: local-ide.service is not healthy, backup restored"
guest cp -p "$guest_backup" "$guest_config"
restart_healthy || fail "local-ide.service is not healthy after the restore of $guest_backup"
fail "the new config did not start, $guest_backup is restored"
