#!/usr/bin/env bash
set -euo pipefail

host_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
release_dir=$(dirname "$host_dir")
# shellcheck source=container_defaults.sh
source "${host_dir}/container_defaults.sh"
# shellcheck source=deploy_steps.sh
source "${host_dir}/deploy_steps.sh"
# shellcheck source=engine_config.sh
source "${host_dir}/engine_config.sh"

[[ $# -eq 1 ]] || fail "usage: install_release.sh <release archive>"
archive=$1
[[ -f $archive ]] || fail "release archive $archive is absent"
version=$(cat "${release_dir}/VERSION")
[[ $version =~ ^[0-9A-Za-z.+-]+$ ]] || fail "release version '$version' is not valid"

guest() {
  pct exec "$CT_ID" -- "$@"
}

user_present() {
  guest id -u "$GUEST_USER" >/dev/null 2>&1
}

create_user() {
  guest useradd --system --user-group --no-create-home \
    --home-dir "$GUEST_DATA_DIR" --shell /usr/sbin/nologin "$GUEST_USER"
}

directories_present() {
  guest test -d "$GUEST_INSTALL_DIR" -a -d "$GUEST_CONFIG_DIR"
}

create_directories() {
  guest install -d -m 755 -o root -g root "$GUEST_INSTALL_DIR"
  guest install -d -m 750 -o root -g "$GUEST_USER" "$GUEST_CONFIG_DIR"
}

release_present() {
  [[ $(guest cat "${GUEST_INSTALL_DIR}/VERSION" 2>/dev/null || true) == "$version" ]]
}

install_files() {
  pct push "$CT_ID" "$archive" "${GUEST_INSTALL_DIR}/release.tar.gz" --perms 600
  # The paths are fixed constants, so the guest shell receives no variable input
  guest sh -euc '
    cd /opt/local-ide
    rm -rf staging web.previous
    mkdir staging
    tar -xzf release.tar.gz -C staging --strip-components=1 --no-same-owner
    install -m 755 staging/local-ide local-ide.next
    mv -f local-ide.next local-ide
    if [ -d web ]; then mv web web.previous; fi
    mv staging/web web
    chmod -R u=rwX,go=rX web
    install -m 644 staging/VERSION VERSION
    rm -rf staging web.previous release.tar.gz
  '
}

install_config() {
  local candidate target gid
  candidate="${DEPLOY_STATE_DIR}/config.toml.candidate"
  derive_engine_config "${release_dir}/config/local-ide.example.toml" \
    "${CT_ADDRESS}:${ENGINE_PORT}" "${PUBLIC_URL%/}" "$CT_GATEWAY" \
    "$GUEST_DATA_DIR" "${GUEST_INSTALL_DIR}/web" "$INFERENCE_BASE_URL" >"$candidate" ||
    fail "cannot derive the engine config"
  target="${GUEST_CONFIG_DIR}/config.toml"
  if guest test -e "$target"; then
    target="${target}.new"
    log "config: existing config.toml kept, candidate written to $target"
  fi
  gid=$(guest id -g "$GUEST_USER")
  pct push "$CT_ID" "$candidate" "$target" --perms 640 --user 0 --group "$gid"
  rm -f "$candidate"
}

key_present() {
  guest grep -q "^${KEY_VARIABLE}=." "${GUEST_CONFIG_DIR}/key.env" 2>/dev/null
}

# The key passes only through this pipe, never through an argument, a log, or a host file
copy_key() {
  pct exec "$INFERENCE_CT_ID" -- grep -m 1 "^${KEY_VARIABLE}=." "$INFERENCE_KEY_FILE" |
    guest sh -euc '
      umask 077
      cat >/etc/local-ide/key.env.next
      if [ ! -s /etc/local-ide/key.env.next ]; then
        rm -f /etc/local-ide/key.env.next
        exit 1
      fi
      chown root:root /etc/local-ide/key.env.next
      mv -f /etc/local-ide/key.env.next /etc/local-ide/key.env
    ' || fail "cannot copy the ${KEY_VARIABLE} line from container ${INFERENCE_CT_ID}"
}

install_unit() {
  pct push "$CT_ID" "${release_dir}/systemd/local-ide.service" /etc/systemd/system/local-ide.service --perms 644
  guest systemctl daemon-reload
  guest systemctl enable local-ide.service
}

restart_service() {
  guest systemctl restart local-ide.service
  sleep 2
  guest systemctl is-active --quiet local-ide.service || fail "local-ide.service is not active after restart"
}

require_container_apply
require_preconditions
container_exists "$CT_ID" || fail "container $CT_ID is absent: run create_container.sh first"
state_init
if [[ $(state_value target) != "$version" ]]; then
  log "install: target version $version, install steps run again"
  state_forget_steps install
  state_set target "$version"
fi
wait_for_guest
run_step install.user user_present create_user
run_step install.directories directories_present create_directories
run_step install.files release_present install_files
run_step install.config absent install_config
run_step install.key key_present copy_key
run_step install.unit absent install_unit
run_step install.restart absent restart_service
state_set installed "$version"
