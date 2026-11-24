# shellcheck shell=bash disable=SC2034
repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
RELEASE_VERSION=$(awk '
  /^\[/ { section = $0 }
  section == "[workspace.package]" && /^version[[:space:]]*=/ {
    gsub(/^[^"]*"|".*$/, "")
    print
    exit
  }
' "${repository_root}/Cargo.toml")
[[ $RELEASE_VERSION =~ ^[0-9A-Za-z.+-]+$ ]] || {
  printf 'cannot read [workspace.package] version from Cargo.toml\n' >&2
  exit 1
}
RELEASE_TARGET=x86_64-unknown-linux-musl
RELEASE_NAME="local-ide-${RELEASE_VERSION}-x86_64-linux"
DIST_DIR="${repository_root}/dist"
ARCHIVE_PATH="${DIST_DIR}/${RELEASE_NAME}.tar.gz"
CHECKSUM_PATH="${ARCHIVE_PATH}.sha256"
