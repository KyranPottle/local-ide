#!/usr/bin/env bash
set -euo pipefail

deploy_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=release_name.sh
source "${deploy_dir}/release_name.sh"
# shellcheck source=host/container_defaults.sh
source "${deploy_dir}/host/container_defaults.sh"
# shellcheck source=host/deploy_steps.sh
source "${deploy_dir}/host/deploy_steps.sh"

check_only=false
apply=false
case ${1:-} in
  --check) check_only=true ;;
  --apply) apply=true ;;
  *) fail "usage: deploy_to_container.sh --check or --apply" ;;
esac

require_container_values
release_root="${DEPLOY_STATE_DIR}/releases"
remote_archive="${release_root}/$(basename "$ARCHIVE_PATH")"
remote_release_dir="${release_root}/${RELEASE_NAME}"

remote_environment="env"
for name in "${CONTAINER_VARIABLES[@]}"; do
  remote_environment+=" ${name}=$(printf '%q' "${!name}")"
done
if $apply; then
  remote_environment+=" LOCAL_IDE_APPLY_CONTAINER=1"
fi

remote() {
  if $check_only; then
    printf 'would run on %s: %s\n' "$PROXMOX_SSH_HOST" "$1"
    return
  fi
  ssh "$PROXMOX_SSH_HOST" "$1"
}

report() {
  $check_only || log "$@"
}

remote_probe() {
  if $check_only; then
    printf 'would check on %s: %s\n' "$PROXMOX_SSH_HOST" "$1"
    return 1
  fi
  ssh "$PROXMOX_SSH_HOST" "$1" >/dev/null 2>&1
}

[[ -f $ARCHIVE_PATH && -f $CHECKSUM_PATH ]] || fail "$ARCHIVE_PATH is absent: run deploy/build_release.sh"
(cd "$DIST_DIR" && shasum -a 256 -c "$(basename "$CHECKSUM_PATH")" >/dev/null) || fail "local checksum does not verify"
log "archive: $ARCHIVE_PATH verified"

remote "install -d -m 700 $(printf '%q' "$release_root")"

verify_remote="cd $(printf '%q' "$release_root") && sha256sum -c $(printf '%q' "$(basename "$CHECKSUM_PATH")")"
if remote_probe "$verify_remote"; then
  log "copy: archive on $PROXMOX_SSH_HOST is current, skipped"
elif $check_only; then
  printf 'would copy %s and %s to %s:%s/\n' "$ARCHIVE_PATH" "$CHECKSUM_PATH" "$PROXMOX_SSH_HOST" "$release_root"
else
  scp -q "$ARCHIVE_PATH" "$CHECKSUM_PATH" "${PROXMOX_SSH_HOST}:${release_root}/"
  report "copy: done"
fi
remote "$verify_remote" || fail "checksum does not verify on $PROXMOX_SSH_HOST"
report "checksum: verified on $PROXMOX_SSH_HOST"

if remote_probe "test -f $(printf '%q' "${remote_release_dir}/VERSION")"; then
  log "extract: $remote_release_dir present, skipped"
else
  remote "tar -xzf $(printf '%q' "$remote_archive") -C $(printf '%q' "$release_root") --no-same-owner"
  report "extract: done"
fi

host_scripts="$(printf '%q' "${remote_release_dir}/host")"
remote "$remote_environment bash ${host_scripts}/create_container.sh"
remote "$remote_environment bash ${host_scripts}/install_release.sh $(printf '%q' "$remote_archive")"
remote "$remote_environment bash ${host_scripts}/publish_tailnet.sh"
remote "$remote_environment bash ${host_scripts}/check_deployment.sh"
report "deploy: done"
