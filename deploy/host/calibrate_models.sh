#!/usr/bin/env bash
set -euo pipefail

host_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=container_defaults.sh
source "${host_dir}/container_defaults.sh"
# shellcheck source=deploy_steps.sh
source "${host_dir}/deploy_steps.sh"

guest_proposal="${GUEST_DATA_DIR}/calibration-proposal.toml"
host_proposal="${PWD}/calibration-proposal.toml"

for argument in "$@"; do
  # Arguments reach the guest command line
  [[ $argument =~ ^[A-Za-z0-9._:/@+=-]+$ ]] || fail "argument '$argument' has characters outside [A-Za-z0-9._:/@+=-]"
done

guest() {
  pct exec "$CT_ID" -- "$@"
}

require_container_apply
require_preconditions
container_exists "$CT_ID" || fail "container $CT_ID is absent: run create_container.sh first"
wait_for_guest
guest test -x "${GUEST_INSTALL_DIR}/local-ide" || fail "${GUEST_INSTALL_DIR}/local-ide is absent: run install_release.sh first"

trap 'if guest systemctl start local-ide.service; then log "service: local-ide.service started"; else log "error: cannot start local-ide.service"; fi' EXIT
guest systemctl stop local-ide.service
log "service: local-ide.service stopped"
guest rm -f "$guest_proposal"

# systemd reads the key file as it does for the service, so the key stays out of each argument and log
log "calibration: started, a model load can take more than a minute"
status=0
guest systemd-run --quiet --wait --pipe --collect --service-type=exec \
  --property=User="$GUEST_USER" --property=Group="$GUEST_USER" \
  --property=EnvironmentFile="${GUEST_CONFIG_DIR}/key.env" --property=UMask=0077 \
  "${GUEST_INSTALL_DIR}/local-ide" calibrate --config "${GUEST_CONFIG_DIR}/config.toml" \
  --output "$guest_proposal" "$@" || status=$?
log "calibration: exit code $status"

if guest test -f "$guest_proposal"; then
  pct pull "$CT_ID" "$guest_proposal" "$host_proposal"
  chmod 600 "$host_proposal"
  log "proposal: copied to $host_proposal"
  printf '%s\n' "$host_proposal"
else
  log "proposal: absent in container $CT_ID"
fi
exit "$status"
