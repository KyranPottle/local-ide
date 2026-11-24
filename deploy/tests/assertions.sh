# shellcheck shell=bash

assertion_count=0
failure_count=0

record_result() {
  assertion_count=$((assertion_count + 1))
  if [[ $2 -eq 0 ]]; then
    printf 'pass: %s\n' "$1"
    return
  fi
  failure_count=$((failure_count + 1))
  printf 'FAIL: %s\n' "$1"
}

assert_true() {
  local status=0
  "${@:2}" >/dev/null 2>&1 || status=$?
  record_result "$1" "$status"
}

assert_equal() {
  local status=0
  [[ $2 == "$3" ]] || status=1
  record_result "$1" "$status"
  [[ $status -eq 0 ]] || diff <(printf '%s\n' "$2") <(printf '%s\n' "$3") | sed 's/^/  /' || true
}

assert_file_contains() {
  local status=0
  grep -qF -- "$2" "$3" 2>/dev/null || status=1
  record_result "$1" "$status"
}

assert_file_lacks() {
  local status=0
  ! grep -qE -- "$2" "$3" 2>/dev/null || status=1
  record_result "$1" "$status"
}

assertion_summary() {
  printf '%d assertions, %d failures\n' "$assertion_count" "$failure_count"
  [[ $failure_count -eq 0 ]]
}
