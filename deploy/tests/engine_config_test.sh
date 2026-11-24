#!/usr/bin/env bash
set -euo pipefail

tests_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
example="${tests_dir}/../../config/local-ide.example.toml"
# shellcheck source=../host/engine_config.sh
source "${tests_dir}/../host/engine_config.sh"

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT
failures=0

expect() {
  if "${@:2}"; then
    printf 'pass: %s\n' "$1"
  else
    printf 'FAIL: %s\n' "$1"
    failures=$((failures + 1))
  fi
}

derive_site() {
  derive_engine_config "$1" 192.0.2.11:7878 https://ide.example.ts.net:8443 192.0.2.1 \
    /var/lib/local-ide /opt/local-ide/web http://192.0.2.10:8080/v1
}

has_line() {
  grep -qxF "$1" "${work_dir}/site.toml"
}

profiles_unchanged() {
  diff <(sed -n '/^\[\[model\]\]/,$p' "$example") <(sed -n '/^\[\[model\]\]/,$p' "${work_dir}/site.toml") >/dev/null
}

line_count_unchanged() {
  [[ $(wc -l <"$example") -eq $(wc -l <"${work_dir}/site.toml") ]]
}

fails() {
  ! "$@" >/dev/null 2>&1
}

derive_example() {
  derive_site "$example" >"${work_dir}/site.toml"
}

expect "derivation succeeds" derive_example
expect "listen replaced" has_line 'listen = "192.0.2.11:7878"'
expect "public_origin replaced" has_line 'public_origin = "https://ide.example.ts.net:8443"'
expect "allowed_peers replaced" has_line 'allowed_peers = ["192.0.2.1"]'
expect "data_dir replaced" has_line 'data_dir = "/var/lib/local-ide"'
expect "assets_dir replaced" has_line 'assets_dir = "/opt/local-ide/web"'
expect "inference base_url replaced" has_line 'base_url = "http://192.0.2.10:8080/v1"'
expect "api_key_env kept" has_line 'api_key_env = "LLAMA_SWAP_KEY"'
expect "model profiles unchanged" profiles_unchanged
expect "no line added or removed" line_count_unchanged

grep -v '^public_origin' "$example" >"${work_dir}/no_origin.toml"
expect "absent top-level key fails" fails derive_site "${work_dir}/no_origin.toml"
sed '/^\[inference\]/,/^\[\[model/{/^base_url/d;}' "$example" >"${work_dir}/no_base_url.toml"
expect "absent inference base_url fails" fails derive_site "${work_dir}/no_base_url.toml"
expect "quote in a value fails" fails derive_engine_config "$example" '1.2.3.4:1" x = "y' o p d a b

[[ $failures -eq 0 ]] || exit 1
