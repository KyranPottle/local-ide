#!/usr/bin/env bash
set -euo pipefail

host_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=container_defaults.sh
source "${host_dir}/container_defaults.sh"
# shellcheck source=deploy_steps.sh
source "${host_dir}/deploy_steps.sh"

resolve_storage() {
  if [[ -n $STORAGE_ID ]]; then
    printf '%s' "$STORAGE_ID"
    return
  fi
  local storage
  storage=$(pct config "$INFERENCE_CT_ID" | sed -n 's/^rootfs: \([^:,]*\):.*/\1/p')
  [[ -n $storage ]] || fail "cannot read the storage id from the rootfs line of container $INFERENCE_CT_ID"
  printf '%s' "$storage"
}

present_templates() {
  pveam list "$TEMPLATE_STORAGE" | awk '{ print $1 }' | grep '/debian-13-standard_.*_amd64\.tar\.'
}

newest_indexed_template() {
  pveam update >&2 || return 1
  pveam available --section system | awk '{ print $2 }' | grep '^debian-13-standard_.*_amd64\.tar\.' | sort -V | tail -n 1
}

resolve_template() {
  if [[ -n $TEMPLATE ]]; then
    printf '%s:vztmpl/%s' "$TEMPLATE_STORAGE" "$TEMPLATE"
    return
  fi
  local name volume
  if name=$(newest_indexed_template) && [[ -n $name ]]; then
    volume="${TEMPLATE_STORAGE}:vztmpl/${name}"
    if ! present_templates | grep -qxF "$volume"; then
      log "downloading template $name to $TEMPLATE_STORAGE"
      pveam download "$TEMPLATE_STORAGE" "$name" >&2 || fail "cannot download template $name"
      present_templates | grep -qxF "$volume" || fail "template $name is absent after the download"
    fi
    printf '%s' "$volume"
    return
  fi
  volume=$(present_templates | sort -V | tail -n 1 || true)
  [[ -n $volume ]] || fail "the template index cannot be read and $TEMPLATE_STORAGE holds no debian-13-standard template"
  log "template index cannot be read, fallback to the newest present template $volume"
  printf '%s' "$volume"
}

container_present() {
  container_exists "$CT_ID"
}

create_container() {
  local storage template
  storage=$(resolve_storage)
  template=$(resolve_template)
  log "creating container $CT_ID from $template on storage $storage"
  pct create "$CT_ID" "$template" \
    --hostname "$CT_HOSTNAME" \
    --unprivileged 1 \
    --features nesting=1 \
    --ostype debian \
    --cores "$CT_CORES" \
    --memory "$CT_MEMORY_MIB" \
    --swap "$CT_SWAP_MIB" \
    --rootfs "${storage}:${CT_DISK_GIB}" \
    --net0 "name=eth0,bridge=${CT_BRIDGE},ip=${CT_ADDRESS}/${CT_PREFIX},gw=${CT_GATEWAY}" \
    --nameserver "$CT_NAMESERVER" \
    --onboot 1
}

start_container() {
  if pct status "$CT_ID" | grep -q 'status: running'; then
    log "container $CT_ID: running"
    return
  fi
  log "container $CT_ID: starting"
  pct start "$CT_ID"
}

require_container_apply
require_preconditions
state_init
run_step create.container container_present create_container
start_container
wait_for_guest
log "container $CT_ID: ready"
