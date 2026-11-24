#!/usr/bin/env bash
set -euo pipefail

deploy_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=release_name.sh
source "${deploy_dir}/release_name.sh"
# shellcheck source=host/deploy_steps.sh
source "${deploy_dir}/host/deploy_steps.sh"

binary_path="${repository_root}/target/${RELEASE_TARGET}/release/local-ide"
web_build_dir="${repository_root}/web/dist"
stage_dir="${DIST_DIR}/stage/${RELEASE_NAME}"

check_tools() {
  local tool
  for tool in cargo cargo-zigbuild zig pnpm rustup shasum tar file; do
    command -v "$tool" >/dev/null || fail "$tool is absent"
  done
  rustup target list --installed | grep -qx "$RELEASE_TARGET" ||
    fail "rust target $RELEASE_TARGET is absent: run rustup target add $RELEASE_TARGET"
  log "tools: present"
}

build_web() {
  log "web: building"
  pnpm --dir "${repository_root}/web" build >&2
  [[ -f ${web_build_dir}/index.html ]] || fail "web build has no index.html"
  log "web: built"
}

build_binary() {
  log "binary: building for $RELEASE_TARGET"
  (cd "$repository_root" && cargo zigbuild --release --locked -p engine --bin local-ide --target "$RELEASE_TARGET" >&2)
  file "$binary_path" | grep -q 'ELF 64-bit LSB.*x86-64.*statically linked' ||
    fail "$binary_path is not a static x86-64 ELF: $(file -b "$binary_path")"
  log "binary: built, $(wc -c <"$binary_path" | tr -d ' ') bytes"
}

archive_current() {
  [[ -f $ARCHIVE_PATH && -f $CHECKSUM_PATH ]] || return 1
  (cd "$DIST_DIR" && shasum -a 256 -c "$(basename "$CHECKSUM_PATH")" >/dev/null 2>&1) || return 1
  [[ -z $(find "$binary_path" "$web_build_dir" "$deploy_dir/local-ide.service" "$deploy_dir/host" \
    "${repository_root}/config/local-ide.example.toml" -newer "$ARCHIVE_PATH" -print -quit) ]]
}

make_archive() {
  if archive_current; then
    log "archive: $ARCHIVE_PATH is current, skipped"
    return
  fi
  rm -rf "${DIST_DIR}/stage"
  mkdir -p "${stage_dir}/config" "${stage_dir}/systemd" "${stage_dir}/host"
  install -m 755 "$binary_path" "${stage_dir}/local-ide"
  cp -R "$web_build_dir" "${stage_dir}/web"
  install -m 644 "${repository_root}/config/local-ide.example.toml" "${stage_dir}/config/"
  install -m 644 "${deploy_dir}/local-ide.service" "${stage_dir}/systemd/"
  install -m 755 "${deploy_dir}"/host/*.sh "${stage_dir}/host/"
  printf '%s\n' "$RELEASE_VERSION" >"${stage_dir}/VERSION"
  COPYFILE_DISABLE=1 tar --no-mac-metadata --no-xattrs --uid 0 --gid 0 -czf "$ARCHIVE_PATH" -C "${DIST_DIR}/stage" "$RELEASE_NAME"
  (cd "$DIST_DIR" && shasum -a 256 "$(basename "$ARCHIVE_PATH")" >"$CHECKSUM_PATH")
  rm -rf "${DIST_DIR}/stage"
  log "archive: $ARCHIVE_PATH, $(wc -c <"$ARCHIVE_PATH" | tr -d ' ') bytes"
}

check_tools
build_web
build_binary
make_archive
