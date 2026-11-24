# shellcheck shell=bash
# Model profiles keep one source in the example config

derive_engine_config() {
  local example=$1 value
  shift
  for value in "$@"; do
    [[ $value =~ ^[A-Za-z0-9._:/@+-]+$ ]] || {
      printf 'derive_engine_config: value %q is not a plain TOML string\n' "$value" >&2
      return 1
    }
  done
  CONFIG_LISTEN=$1 CONFIG_PUBLIC_ORIGIN=$2 CONFIG_ALLOWED_PEER=$3 \
    CONFIG_DATA_DIR=$4 CONFIG_ASSETS_DIR=$5 CONFIG_BASE_URL=$6 \
    awk '
      BEGIN {
        replacement["|listen"] = "\"" ENVIRON["CONFIG_LISTEN"] "\""
        replacement["|public_origin"] = "\"" ENVIRON["CONFIG_PUBLIC_ORIGIN"] "\""
        replacement["|allowed_peers"] = "[\"" ENVIRON["CONFIG_ALLOWED_PEER"] "\"]"
        replacement["|data_dir"] = "\"" ENVIRON["CONFIG_DATA_DIR"] "\""
        replacement["|assets_dir"] = "\"" ENVIRON["CONFIG_ASSETS_DIR"] "\""
        replacement["[inference]|base_url"] = "\"" ENVIRON["CONFIG_BASE_URL"] "\""
      }
      /^[[:space:]]*\[/ {
        section = $0
        gsub(/[[:space:]]/, "", section)
        print
        next
      }
      /^[[:space:]]*[A-Za-z_]+[[:space:]]*=/ {
        key = $0
        sub(/^[[:space:]]*/, "", key)
        sub(/[[:space:]]*=.*/, "", key)
        if ((section "|" key) in replacement) {
          print key " = " replacement[section "|" key]
          replaced[section "|" key] = 1
          next
        }
      }
      { print }
      END {
        for (id in replacement) {
          if (!(id in replaced)) {
            printf "derive_engine_config: key %s is absent\n", id > "/dev/stderr"
            missing = 1
          }
        }
        exit missing
      }
    ' "$example"
}
