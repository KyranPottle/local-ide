# shellcheck shell=bash

record_call() {
  local line
  line=$(printf '%q ' "$(basename "$0")" "$@")
  line=${line% }
  line=${line//"$SANDBOX"/SANDBOX}
  printf '%s\n' "$line" >>"${SANDBOX}/calls.log"
  local injected="${SANDBOX}/scenario/fail_once"
  if [[ -f $injected && $line == *"$(cat "$injected")"* ]]; then
    rm -f "$injected"
    printf '%s: injected failure\n' "$(basename "$0")" >&2
    exit 1
  fi
}

scenario_value() {
  cat "${SANDBOX}/scenario/$1" 2>/dev/null || printf '%s' "$2"
}
