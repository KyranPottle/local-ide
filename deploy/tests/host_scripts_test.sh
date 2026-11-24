#!/usr/bin/env bash
set -euo pipefail

# Container tests stay out of the default checks
[[ ${LOCAL_IDE_CONTAINER_TESTS:-} == 1 ]] || {
  printf 'container tests skipped\n'
  exit 0
}

tests_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
deploy_dir=$(dirname "$tests_dir")
repository_root=$(dirname "$deploy_dir")
# shellcheck source=assertions.sh
source "${tests_dir}/assertions.sh"

# Must match the value that the pct stub prints for the key line of CT 200
inference_key=sk-fake-4b1d9e72c0
template_volume=local:vztmpl/debian-13-standard_13.1-2_amd64.tar.zst

root=$(cd "$(mktemp -d)" && pwd -P)
trap 'rm -rf "$root"' EXIT
tools_dir="${root}/tools"
SANDBOX=""
release_dir=""
archive=""
call_mark=0
exit_status=0
deployment_statuses=""
repository=""

# The scripts see only the stubs and these tools, so a real pct or tailscale on this machine stays out of reach
make_tools() {
  local tool real_sed
  mkdir -p "$tools_dir"
  for tool in awk basename bash cat chmod cmp cp date diff dirname env grep gzip head install mkdir mktemp mv perl rm seq shasum sort tail tar touch tr wc; do
    ln -s "$(command -v "$tool")" "${tools_dir}/${tool}"
  done
  printf '#!/bin/sh\nexit 0\n' >"${tools_dir}/sleep"
  real_sed=$(command -v sed)
  if sed --version >/dev/null 2>&1; then
    ln -s "$real_sed" "${tools_dir}/sed"
  else
    # The host has BSD sed, whose -i needs a suffix
    cat >"${tools_dir}/sed" <<EOF
#!/bin/sh
if [ "\$1" = -i ]; then shift; exec ${real_sed} -i '' "\$@"; fi
exec ${real_sed} "\$@"
EOF
  fi
  chmod 755 "${tools_dir}/sleep" "${tools_dir}/sed"
}

make_release() {
  local name="local-ide-$1-x86_64-linux"
  release_dir="${SANDBOX}/releases/${name}"
  mkdir -p "${release_dir}/host" "${release_dir}/config" "${release_dir}/systemd" "${release_dir}/web"
  cp "${deploy_dir}"/host/*.sh "${release_dir}/host/"
  cp "${repository_root}/config/local-ide.example.toml" "${release_dir}/config/"
  cp "${deploy_dir}/local-ide.service" "${release_dir}/systemd/"
  printf '#!/bin/sh\n' >"${release_dir}/local-ide"
  printf '<!doctype html>\n' >"${release_dir}/web/index.html"
  printf '%s\n' "$1" >"${release_dir}/VERSION"
  archive="${release_dir}.tar.gz"
  COPYFILE_DISABLE=1 tar -czf "$archive" -C "${SANDBOX}/releases" "$name"
}

new_sandbox() {
  SANDBOX=$(mktemp -d "${root}/case.XXXXXX")
  local scenario="${SANDBOX}/scenario" stub
  mkdir -p "${SANDBOX}/bin" "${SANDBOX}/work" "${SANDBOX}/out" "${SANDBOX}/pve/lxc" "${scenario}/containers/200"
  for stub in "${tests_dir}"/host_stubs/*; do
    ln -s "$stub" "${SANDBOX}/bin/"
  done
  touch "${SANDBOX}/calls.log"
  printf 'hostname: inference\nrootfs: local-lvm:subvol-200-disk-0,size=32G\nnet0: name=eth0,bridge=vmbr0,ip=192.0.2.10/24\n' \
    >"${scenario}/containers/200/config"
  printf 'running\n' >"${scenario}/containers/200/status"
  cp "${scenario}/containers/200/config" "${SANDBOX}/pve/lxc/200.conf"
  printf 'NAME SIZE\n%s 123.81MB\n' "$template_volume" >"${scenario}/templates"
  printf 'system          debian-13-standard_13.1-2_amd64.tar.zst\n' >"${scenario}/available"
  printf '{"AllowFunnel":{"ide.example.ts.net:443":true},"TCP":{"443":{"HTTPS":true}},"Web":{"ide.example.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:8080"}}}}}\n' \
    >"${scenario}/serve.json"
  printf 'ok' >"${scenario}/health"
  printf '[{"id":"coder","available":true},{"id":"fast","available":false}]\n' >"${scenario}/models"
  make_release 0.1.0
}

run_script() {
  local script=$1
  local environment=(PATH="${SANDBOX}/bin:${tools_dir}" HOME="${SANDBOX}/work" SANDBOX="$SANDBOX"
    DEPLOY_STATE_DIR="${SANDBOX}/state" PVE_CONFIG_DIR="${SANDBOX}/pve" LOCAL_IDE_APPLY_CONTAINER=1)
  shift
  while [[ $1 != -- ]]; do
    environment+=("$1")
    shift
  done
  shift
  exit_status=0
  (cd "${SANDBOX}/work" && env -i "${environment[@]}" bash "$script" "$@") \
    >"${SANDBOX}/out/stdout" 2>"${SANDBOX}/out/stderr" || exit_status=$?
  cat "${SANDBOX}/out/stdout" >>"${SANDBOX}/out/stdout.all"
  cat "${SANDBOX}/out/stderr" >>"${SANDBOX}/out/stderr.all"
}

run_host() {
  local script=$1
  shift
  run_script "${release_dir}/host/${script}" "$@"
}

run_deployment() {
  local statuses=""
  run_host create_container.sh --
  statuses+="$exit_status "
  run_host install_release.sh -- "$archive"
  statuses+="$exit_status "
  run_host publish_tailnet.sh --
  statuses+="$exit_status "
  run_host check_deployment.sh --
  statuses+="$exit_status"
  deployment_statuses=$statuses
}

mark_calls() {
  call_mark=$(wc -l <"${SANDBOX}/calls.log")
}

state_changes() {
  tail -n "+$((call_mark + 1))" "${SANDBOX}/calls.log" |
    grep -E '^pct (create|start|stop|push|pull) |^pct exec 201 -- (useradd|install|sh|rm|cp|mv|systemd-run|systemctl (daemon-reload|enable|disable|start|stop|restart))( |$)|^pveam (update|download)|^tailscale (serve [^s]|funnel)' |
    sed -E -e 's/ sh -euc .*key\.env\.next.*/ sh -euc KEY_SCRIPT/' -e 's/ sh -euc .*release\.tar\.gz.*/ sh -euc RELEASE_SCRIPT/' ||
    true
}

guest_file() {
  printf '%s/guest/201%s' "$SANDBOX" "$1"
}

test_first_deploy() {
  new_sandbox
  run_deployment
  assert_equal "first deploy: each script exits 0" "0 0 0 0" "$deployment_statuses"
  assert_equal "first deploy: state-changing calls" "$(
    cat <<'EOF'
pveam update
pct create 201 local:vztmpl/debian-13-standard_13.1-2_amd64.tar.zst --hostname local-ide --unprivileged 1 --features nesting=1 --ostype debian --cores 2 --memory 256 --swap 0 --rootfs local-lvm:2 --net0 name=eth0\,bridge=vmbr0\,ip=192.0.2.11/24\,gw=192.0.2.1 --nameserver 1.1.1.1 --onboot 1
pct start 201
pct exec 201 -- useradd --system --user-group --no-create-home --home-dir /var/lib/local-ide --shell /usr/sbin/nologin local-ide
pct exec 201 -- install -d -m 755 -o root -g root /opt/local-ide
pct exec 201 -- install -d -m 750 -o root -g local-ide /etc/local-ide
pct push 201 SANDBOX/releases/local-ide-0.1.0-x86_64-linux.tar.gz /opt/local-ide/release.tar.gz --perms 600
pct exec 201 -- sh -euc RELEASE_SCRIPT
pct push 201 SANDBOX/state/config.toml.candidate /etc/local-ide/config.toml --perms 640 --user 0 --group 999
pct exec 201 -- sh -euc KEY_SCRIPT
pct push 201 SANDBOX/releases/local-ide-0.1.0-x86_64-linux/systemd/local-ide.service /etc/systemd/system/local-ide.service --perms 644
pct exec 201 -- systemctl daemon-reload
pct exec 201 -- systemctl enable local-ide.service
pct exec 201 -- systemctl restart local-ide.service
tailscale serve --bg --https=8443 http://192.0.2.11:7878
EOF
  )" "$(state_changes)"
  assert_file_contains "first deploy: health check" 'curl -fsS --max-time 5 http://192.0.2.11:7878/api/health' "${SANDBOX}/calls.log"
  assert_equal "first deploy: check prints the URL" "https://ide.example.ts.net:8443/" "$(cat "${SANDBOX}/out/stdout")"
  assert_file_contains "first deploy: config listen" 'listen = "192.0.2.11:7878"' "$(guest_file /etc/local-ide/config.toml)"
  assert_file_contains "first deploy: config origin" 'public_origin = "https://ide.example.ts.net:8443"' "$(guest_file /etc/local-ide/config.toml)"
  assert_equal "first deploy: key line in CT 201" "LLAMA_SWAP_KEY=${inference_key}" "$(cat "$(guest_file /etc/local-ide/key.env)")"
  assert_equal "first deploy: installed version" "0.1.0" "$(cat "$(guest_file /opt/local-ide/VERSION)")"
  assert_file_contains "first deploy: state records installed version" "installed 0.1.0" "${SANDBOX}/state/state"
  assert_file_contains "first deploy: Funnel on 443 kept" '"AllowFunnel":{"ide.example.ts.net:443":true}' "${SANDBOX}/scenario/serve.json"
  assert_true "first deploy: no candidate left on the host" test ! -e "${SANDBOX}/state/config.toml.candidate"
}

test_unchanged_rerun() {
  new_sandbox
  run_deployment
  mark_calls
  run_deployment
  assert_equal "rerun: each script exits 0" "0 0 0 0" "$deployment_statuses"
  assert_equal "rerun: no state-changing call" "" "$(state_changes)"
  assert_file_contains "rerun: install steps skipped from the state file" "install.restart: recorded as complete, skipped" "${SANDBOX}/out/stderr.all"
  printf 'stopped\n' >"${SANDBOX}/scenario/containers/201/status"
  mark_calls
  run_host create_container.sh --
  assert_equal "rerun with stopped container: only the start" "pct start 201" "$(state_changes)"
}

test_upgrade() {
  new_sandbox
  run_deployment
  printf '# owner edit\n' >>"$(guest_file /etc/local-ide/config.toml)"
  cp "$(guest_file /etc/local-ide/config.toml)" "${SANDBOX}/config.before"
  make_release 0.2.0
  mark_calls
  run_deployment
  assert_equal "upgrade: each script exits 0" "0 0 0 0" "$deployment_statuses"
  assert_equal "upgrade: state-changing calls" "$(
    cat <<'EOF'
pct push 201 SANDBOX/releases/local-ide-0.2.0-x86_64-linux.tar.gz /opt/local-ide/release.tar.gz --perms 600
pct exec 201 -- sh -euc RELEASE_SCRIPT
pct push 201 SANDBOX/state/config.toml.candidate /etc/local-ide/config.toml.new --perms 640 --user 0 --group 999
pct push 201 SANDBOX/releases/local-ide-0.2.0-x86_64-linux/systemd/local-ide.service /etc/systemd/system/local-ide.service --perms 644
pct exec 201 -- systemctl daemon-reload
pct exec 201 -- systemctl enable local-ide.service
pct exec 201 -- systemctl restart local-ide.service
EOF
  )" "$(state_changes)"
  assert_true "upgrade: existing config.toml kept" cmp "${SANDBOX}/config.before" "$(guest_file /etc/local-ide/config.toml)"
  assert_file_contains "upgrade: candidate beside it" 'listen = "192.0.2.11:7878"' "$(guest_file /etc/local-ide/config.toml.new)"
  assert_equal "upgrade: installed version" "0.2.0" "$(cat "$(guest_file /opt/local-ide/VERSION)")"
}

expect_refusal() {
  mark_calls
  run_host create_container.sh --
  assert_true "$1: exits non-zero" test "$exit_status" -ne 0
  assert_file_contains "$1: message" "$2" "${SANDBOX}/out/stderr"
  assert_equal "$1: state-changing calls" "${3:-}" "$(state_changes)"
}

test_preconditions() {
  new_sandbox
  rm "${SANDBOX}/bin/pct"
  expect_refusal "pct absent" "pct is absent: run this script on the Proxmox host"

  new_sandbox
  rm -r "${SANDBOX}/scenario/containers/200"
  expect_refusal "CT 200 absent" "container 200 (inference) is absent"

  new_sandbox
  mkdir -p "${SANDBOX}/scenario/containers/201"
  printf 'hostname: web\n' >"${SANDBOX}/scenario/containers/201/config"
  expect_refusal "id 201 in use" "id 201 is in use by container 'web', not 'local-ide'"

  new_sandbox
  printf 'hostname: other\nnet0: name=eth0,bridge=vmbr0,ip=192.0.2.11/24\n' >"${SANDBOX}/pve/lxc/202.conf"
  expect_refusal "address in use" "address 192.0.2.11 is in use"

  new_sandbox
  rm "${SANDBOX}/bin/tailscale"
  expect_refusal "tailscale absent" "tailscale is absent"

  new_sandbox
  printf 'NAME SIZE\n' >"${SANDBOX}/scenario/templates"
  printf '1\n' >"${SANDBOX}/scenario/download_status"
  expect_refusal "template download fails" "cannot download template debian-13-standard_13.1-2_amd64.tar.zst" \
    "$(printf 'pveam update\npveam download local debian-13-standard_13.1-2_amd64.tar.zst')"

  new_sandbox
  printf '{"AllowFunnel":{"ide.example.ts.net:8443":true},"TCP":{"8443":{"HTTPS":true}}}\n' >"${SANDBOX}/scenario/serve.json"
  expect_refusal "Funnel on 8443" "port 8443 has a Funnel"

  new_sandbox
  printf '{"TCP":{"8443":{"HTTPS":true}},"Web":{"ide.example.ts.net:8443":{"Handlers":{"/":{"Proxy":"http://192.0.2.19:80"}}}}}\n' \
    >"${SANDBOX}/scenario/serve.json"
  expect_refusal "other target on 8443" "port 8443 serves another target"
}

expect_template() {
  mark_calls
  run_host create_container.sh "${@:3}" --
  assert_equal "$1: exits 0" 0 "$exit_status"
  assert_equal "$1: state-changing calls" "$2" "$(state_changes | sed -e '/^pct start /d' -e 's/ --hostname .*//')"
}

test_template_selection() {
  local newer=debian-13-standard_13.6-1_amd64.tar.zst
  new_sandbox
  printf 'system          debian-13-standard_13.1-2_amd64.tar.zst\nsystem          %s\nsystem          debian-12-standard_12.12-1_amd64.tar.zst\n' \
    "$newer" >"${SANDBOX}/scenario/available"
  expect_template "newer template in the index" \
    "$(printf 'pveam update\npveam download local %s\npct create 201 local:vztmpl/%s' "$newer" "$newer")"

  new_sandbox
  printf 'system          %s\n' "$newer" >"${SANDBOX}/scenario/available"
  printf 'local:vztmpl/%s 130.00MB\n' "$newer" >>"${SANDBOX}/scenario/templates"
  expect_template "newer template present" "$(printf 'pveam update\npct create 201 local:vztmpl/%s' "$newer")"

  new_sandbox
  printf 'system          %s\n' "$newer" >"${SANDBOX}/scenario/available"
  printf '1\n' >"${SANDBOX}/scenario/update_status"
  expect_template "index unreachable" "$(printf 'pveam update\npct create 201 %s' "$template_volume")"
  assert_file_contains "index unreachable: fallback logged" \
    "template index cannot be read, fallback to the newest present template ${template_volume}" "${SANDBOX}/out/stderr"

  new_sandbox
  printf 'NAME SIZE\n' >"${SANDBOX}/scenario/templates"
  printf '1\n' >"${SANDBOX}/scenario/update_status"
  expect_refusal "index unreachable, no template" "the template index cannot be read and local holds no debian-13-standard template" \
    "pveam update"

  new_sandbox
  printf 'system          %s\n' "$newer" >"${SANDBOX}/scenario/available"
  expect_template "template override" "pct create 201 local:vztmpl/custom.tar.zst" TEMPLATE=custom.tar.zst
}

test_port_443_untouched() {
  cat "$root"/case.*/calls.log >"${root}/all_calls.log"
  assert_file_lacks "no call changes port 443" '^tailscale (serve (reset|off)|funnel)|--https=443( |$)|:443( |$)' "${root}/all_calls.log"
}

test_interrupted_install() {
  new_sandbox
  run_host create_container.sh --
  printf '/etc/systemd/system/local-ide.service' >"${SANDBOX}/scenario/fail_once"
  run_host install_release.sh -- "$archive"
  assert_true "interrupted: install exits non-zero" test "$exit_status" -ne 0
  assert_file_contains "interrupted: key step recorded" "step install.key" "${SANDBOX}/state/state"
  mark_calls
  run_host install_release.sh -- "$archive"
  assert_equal "interrupted: rerun exits 0" 0 "$exit_status"
  assert_equal "interrupted: rerun continues at the unit step" "$(
    cat <<'EOF'
pct push 201 SANDBOX/releases/local-ide-0.1.0-x86_64-linux/systemd/local-ide.service /etc/systemd/system/local-ide.service --perms 644
pct exec 201 -- systemctl daemon-reload
pct exec 201 -- systemctl enable local-ide.service
pct exec 201 -- systemctl restart local-ide.service
EOF
  )" "$(state_changes)"
  assert_file_contains "interrupted: checkpoint logged" "install.files: recorded as complete, skipped" "${SANDBOX}/out/stderr"
}

test_hostile_values() {
  new_sandbox
  # shellcheck disable=SC2016
  expect_refusal_with_value STORAGE_ID 'fast zfs$(touch pwned);x' "STORAGE_ID has characters outside"
  expect_refusal_with_value CT_HOSTNAME "bad';touch pwned;'" "CT_HOSTNAME has characters outside"

  new_sandbox
  # shellcheck disable=SC2016
  sed -i.orig 's/^rootfs: local-lvm:/rootfs: local lvm$(touch pwned):/' "${SANDBOX}/scenario/containers/200/config"
  run_host create_container.sh --
  assert_file_contains "hostile rootfs storage: one argument" '--rootfs local\ lvm\$\(touch\ pwned\):2 ' "${SANDBOX}/calls.log"

  new_sandbox
  local hostile="${SANDBOX}/releases/a b;\$(touch pwned)'\".tar.gz"
  cp "$archive" "$hostile"
  run_host create_container.sh --
  run_host install_release.sh -- "$hostile"
  assert_equal "hostile archive path: install exits 0" 0 "$exit_status"
  assert_file_contains "hostile archive path: one argument" \
    "pct push 201 SANDBOX/releases/a\\ b\\;\\\$\\(touch\\ pwned\\)\\'\\\".tar.gz /opt/local-ide/release.tar.gz --perms 600" "${SANDBOX}/calls.log"
  assert_equal "hostile values: no pwned file" "" "$(find "$root" "$PWD" -maxdepth 3 -name pwned)"
}

expect_refusal_with_value() {
  mark_calls
  run_host create_container.sh "$1=$2" --
  assert_true "hostile $1: exits non-zero" test "$exit_status" -ne 0
  assert_file_contains "hostile $1: message" "$3" "${SANDBOX}/out/stderr"
  assert_equal "hostile $1: no call" "" "$(tail -n "+$((call_mark + 1))" "${SANDBOX}/calls.log")"
}

test_calibration() {
  new_sandbox
  run_deployment
  mark_calls
  run_host calibrate_models.sh -- --model coder --fresh
  assert_equal "calibration: exits 0" 0 "$exit_status"
  assert_equal "calibration: state-changing calls" "$(
    cat <<'EOF'
pct exec 201 -- systemctl stop local-ide.service
pct exec 201 -- rm -f /var/lib/local-ide/calibration-proposal.toml
pct exec 201 -- systemd-run --quiet --wait --pipe --collect --service-type=exec --property=User=local-ide --property=Group=local-ide --property=EnvironmentFile=/etc/local-ide/key.env --property=UMask=0077 /opt/local-ide/local-ide calibrate --config /etc/local-ide/config.toml --output /var/lib/local-ide/calibration-proposal.toml --model coder --fresh
pct pull 201 /var/lib/local-ide/calibration-proposal.toml SANDBOX/work/calibration-proposal.toml
pct exec 201 -- systemctl start local-ide.service
EOF
  )" "$(state_changes)"
  assert_equal "calibration: prints the proposal path" "${SANDBOX}/work/calibration-proposal.toml" "$(cat "${SANDBOX}/out/stdout")"

  printf '3\n' >"${SANDBOX}/scenario/calibration_status"
  mark_calls
  run_host calibrate_models.sh --
  assert_equal "failed calibration: exit code passes through" 3 "$exit_status"
  assert_equal "failed calibration: service starts again" "$(
    cat <<'EOF'
pct exec 201 -- systemctl stop local-ide.service
pct exec 201 -- rm -f /var/lib/local-ide/calibration-proposal.toml
pct exec 201 -- systemd-run --quiet --wait --pipe --collect --service-type=exec --property=User=local-ide --property=Group=local-ide --property=EnvironmentFile=/etc/local-ide/key.env --property=UMask=0077 /opt/local-ide/local-ide calibrate --config /etc/local-ide/config.toml --output /var/lib/local-ide/calibration-proposal.toml
pct exec 201 -- systemctl start local-ide.service
EOF
  )" "$(state_changes)"

  mark_calls
  run_host calibrate_models.sh -- --model 'coder --fresh'
  assert_true "calibration argument with a space: exits non-zero" test "$exit_status" -ne 0
  assert_equal "calibration argument with a space: no call" "" "$(tail -n "+$((call_mark + 1))" "${SANDBOX}/calls.log")"
}

apply_changes() {
  state_changes | sed -E -e 's/apply\.[A-Za-z0-9]+/apply.WORK/g' -e 's/config\.toml\.[0-9]{8}T[0-9]{6}Z/config.toml.STAMP/g'
}

test_apply_calibration() {
  local config proposal backups
  new_sandbox
  run_deployment
  config=$(guest_file /etc/local-ide/config.toml)
  printf 'listen = "192.0.2.11:7878"\n\n[[model]]\nid = "alpha"\ntool_codec = "text"\n\n[[model]]\nid = "beta"\ntool_codec = "text"\n# owner note\n' >"$config"
  proposal="${SANDBOX}/work/proposal.toml"
  printf '[[model]]\nid = "alpha"\ntool_codec = "native"\nchars_per_token = 3.5\n' >"$proposal"
  mark_calls
  run_host apply_calibration.sh -- "$proposal"
  assert_equal "apply: exits 0" 0 "$exit_status"
  assert_equal "apply: state-changing calls" "$(
    cat <<'EOF'
pct pull 201 /etc/local-ide/config.toml SANDBOX/state/apply.WORK/config.toml
pct exec 201 -- cp -p /etc/local-ide/config.toml /etc/local-ide/config.toml.STAMP
pct push 201 SANDBOX/state/apply.WORK/config.next /etc/local-ide/config.toml.next --perms 640 --user 0 --group 999
pct exec 201 -- mv -f /etc/local-ide/config.toml.next /etc/local-ide/config.toml
pct exec 201 -- systemctl restart local-ide.service
EOF
  )" "$(apply_changes)"
  assert_equal "apply: only the alpha table changes" "$(
    printf 'listen = "192.0.2.11:7878"\n\n[[model]]\nid = "alpha"\ntool_codec = "native"\nchars_per_token = 3.5\n\n[[model]]\nid = "beta"\ntool_codec = "text"\n# owner note'
  )" "$(cat "$config")"
  backups=("$(guest_file /etc/local-ide)"/config.toml.2*)
  assert_equal "apply: backup holds the old config" "$(
    printf 'listen = "192.0.2.11:7878"\n\n[[model]]\nid = "alpha"\ntool_codec = "text"\n\n[[model]]\nid = "beta"\ntool_codec = "text"\n# owner note'
  )" "$(cat "${backups[0]}")"
  assert_equal "apply: no work directory left" "" "$(find "${SANDBOX}/state" -name 'apply.*')"

  mark_calls
  run_host apply_calibration.sh -- "$proposal"
  assert_equal "apply again: exits 0" 0 "$exit_status"
  assert_equal "apply again: only the pull" "pct pull 201 /etc/local-ide/config.toml SANDBOX/state/apply.WORK/config.toml" "$(apply_changes)"

  printf '[[model]]\nid = "gamma"\ntool_codec = "native"\n' >"$proposal"
  mark_calls
  run_host apply_calibration.sh -- "$proposal"
  assert_true "unknown model: exits non-zero" test "$exit_status" -ne 0
  assert_file_contains "unknown model: message" "model gamma is absent in the config" "${SANDBOX}/out/stderr"
  assert_equal "unknown model: only the pull" "pct pull 201 /etc/local-ide/config.toml SANDBOX/state/apply.WORK/config.toml" "$(apply_changes)"

  printf '[inference]\nbase_url = "http://192.0.2.19/v1"\n' >"$proposal"
  run_host apply_calibration.sh -- "$proposal"
  assert_true "other table: exits non-zero" test "$exit_status" -ne 0
  assert_file_contains "other table: message" "the proposal holds [inference]" "${SANDBOX}/out/stderr"

  printf '[[model]]\nid = "beta"\ntool_codec = "native"\n' >"$proposal"
  printf 'down' >"${SANDBOX}/scenario/health"
  cp "$config" "${SANDBOX}/config.before"
  mark_calls
  run_host apply_calibration.sh -- "$proposal"
  assert_true "unhealthy: exits non-zero" test "$exit_status" -ne 0
  assert_file_contains "unhealthy: message" "local-ide.service is not healthy, backup restored" "${SANDBOX}/out/stderr"
  assert_true "unhealthy: old config restored" cmp "${SANDBOX}/config.before" "$config"
  assert_equal "unhealthy: restore and restart after the failed restart" "$(
    cat <<'EOF'
pct exec 201 -- systemctl restart local-ide.service
pct exec 201 -- cp -p /etc/local-ide/config.toml.STAMP /etc/local-ide/config.toml
pct exec 201 -- systemctl restart local-ide.service
EOF
  )" "$(apply_changes | tail -n 3)"
}

make_repository() {
  new_sandbox
  repository="${SANDBOX}/repository"
  mkdir -p "${repository}/deploy" "${repository}/dist"
  printf '[workspace.package]\nversion = "0.1.0"\n' >"${repository}/Cargo.toml"
  cp "${deploy_dir}/deploy_to_container.sh" "${deploy_dir}/release_name.sh" "${repository}/deploy/"
  cp -R "${deploy_dir}/host" "${repository}/deploy/host"
  cp "$archive" "${repository}/dist/"
  (cd "${repository}/dist" && shasum -a 256 local-ide-0.1.0-x86_64-linux.tar.gz >local-ide-0.1.0-x86_64-linux.tar.gz.sha256)
}

test_deploy_to_container() {
  local environment
  read -r environment <<'EOF'
env\ PROXMOX_SSH_HOST=build-host\ TAILNET_HOST=ide.example.ts.net\ INFERENCE_CT_ID=200\ INFERENCE_KEY_FILE=/etc/llama-swap/key.env\ INFERENCE_BASE_URL=http://192.0.2.10:8080/v1\ KEY_VARIABLE=LLAMA_SWAP_KEY\ CT_ID=201\ CT_HOSTNAME=local-ide\ CT_ADDRESS=192.0.2.11\ CT_PREFIX=24\ CT_BRIDGE=vmbr0\ CT_GATEWAY=192.0.2.1\ CT_NAMESERVER=1.1.1.1\ CT_CORES=2\ CT_MEMORY_MIB=256\ CT_SWAP_MIB=0\ CT_DISK_GIB=2\ STORAGE_ID=\'\'\ TEMPLATE_STORAGE=local\ TEMPLATE=\'\'\ ENGINE_PORT=7878\ SERVE_PORT=8443\ DEPLOY_STATE_DIR=/var/lib/local-ide-deploy\ LOCAL_IDE_APPLY_CONTAINER=1\ bash\ /var/lib/local-ide-deploy/releases/local-ide-0.1.0-x86_64-linux/host
EOF
  make_repository
  run_script "${repository}/deploy/deploy_to_container.sh" DEPLOY_STATE_DIR=/var/lib/local-ide-deploy --
  assert_true "no argument: exits non-zero" test "$exit_status" -ne 0
  assert_file_contains "no argument: usage" "usage: deploy_to_container.sh --check or --apply" "${SANDBOX}/out/stderr"
  assert_equal "no argument: no call" "" "$(cat "${SANDBOX}/calls.log")"

  make_repository
  run_script "${repository}/deploy/deploy_to_container.sh" DEPLOY_STATE_DIR=/var/lib/local-ide-deploy -- --apply
  assert_equal "deploy_to_container: exits 0" 0 "$exit_status"
  assert_equal "deploy_to_container: copy, verify, then host scripts in order" "$(
    cat <<EOF
ssh build-host install\\ -d\\ -m\\ 700\\ /var/lib/local-ide-deploy/releases
ssh build-host cd\\ /var/lib/local-ide-deploy/releases\\ \\&\\&\\ sha256sum\\ -c\\ local-ide-0.1.0-x86_64-linux.tar.gz.sha256
scp -q SANDBOX/repository/dist/local-ide-0.1.0-x86_64-linux.tar.gz SANDBOX/repository/dist/local-ide-0.1.0-x86_64-linux.tar.gz.sha256 build-host:/var/lib/local-ide-deploy/releases/
ssh build-host cd\\ /var/lib/local-ide-deploy/releases\\ \\&\\&\\ sha256sum\\ -c\\ local-ide-0.1.0-x86_64-linux.tar.gz.sha256
ssh build-host test\\ -f\\ /var/lib/local-ide-deploy/releases/local-ide-0.1.0-x86_64-linux/VERSION
ssh build-host tar\\ -xzf\\ /var/lib/local-ide-deploy/releases/local-ide-0.1.0-x86_64-linux.tar.gz\\ -C\\ /var/lib/local-ide-deploy/releases\\ --no-same-owner
ssh build-host ${environment}/create_container.sh
ssh build-host ${environment}/install_release.sh\\ /var/lib/local-ide-deploy/releases/local-ide-0.1.0-x86_64-linux.tar.gz
ssh build-host ${environment}/publish_tailnet.sh
ssh build-host ${environment}/check_deployment.sh
EOF
  )" "$(cat "${SANDBOX}/calls.log")"

  make_repository
  printf '1\n' >"${SANDBOX}/scenario/checksum_status"
  run_script "${repository}/deploy/deploy_to_container.sh" DEPLOY_STATE_DIR=/var/lib/local-ide-deploy -- --apply
  assert_true "checksum mismatch: exits non-zero" test "$exit_status" -ne 0
  assert_file_contains "checksum mismatch: message" "checksum does not verify on build-host" "${SANDBOX}/out/stderr"
  assert_file_lacks "checksum mismatch: no host script runs" 'host/[a-z_]+\.sh' "${SANDBOX}/calls.log"

  make_repository
  run_script "${repository}/deploy/deploy_to_container.sh" DEPLOY_STATE_DIR=/var/lib/local-ide-deploy -- --check
  assert_equal "check mode: exits 0" 0 "$exit_status"
  assert_equal "check mode: no call" "" "$(cat "${SANDBOX}/calls.log")"
}

test_apply_gate() {
  new_sandbox
  run_script "${release_dir}/host/create_container.sh" LOCAL_IDE_APPLY_CONTAINER= --
  assert_true "create without apply: exits non-zero" test "$exit_status" -ne 0
  assert_file_contains "create without apply: message" "set LOCAL_IDE_APPLY_CONTAINER=1" "${SANDBOX}/out/stderr"
  assert_equal "create without apply: no call" "" "$(cat "${SANDBOX}/calls.log")"
}

test_key_confinement() {
  local leaked
  leaked=$(find "$root" -type f ! -path '*/guest/201/etc/local-ide/key.env' -exec grep -lF "$inference_key" {} + || true)
  assert_equal "key value only in the key file of CT 201" "" "${leaked//"$root"/}"
  assert_true "key value reached CT 201" grep -rqF "$inference_key" "$root" --include=key.env
}

make_tools
test_first_deploy
test_unchanged_rerun
test_upgrade
test_preconditions
test_template_selection
test_interrupted_install
test_hostile_values
test_calibration
test_apply_calibration
test_deploy_to_container
test_apply_gate
test_port_443_untouched
test_key_confinement
printf 'run time: %d s\n' "$SECONDS"
assertion_summary
