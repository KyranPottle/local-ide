#!/usr/bin/env bash
set -euo pipefail

# Container tests stay out of the default checks
[[ ${LOCAL_IDE_CONTAINER_TESTS:-} == 1 ]] || {
  printf 'container tests skipped\n'
  exit 0
}

tests_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=../release_name.sh
source "${tests_dir}/../release_name.sh"
# shellcheck source=../host/engine_config.sh
source "${tests_dir}/../host/engine_config.sh"

image=debian:13-slim
docker info >/dev/null 2>&1 || {
  printf 'docker daemon does not answer\n' >&2
  exit 2
}
[[ -f $ARCHIVE_PATH ]] || {
  printf '%s is absent: run deploy/build_release.sh\n' "$ARCHIVE_PATH" >&2
  exit 1
}

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT
tar -xzf "$ARCHIVE_PATH" -C "$work_dir"
derive_engine_config "${work_dir}/${RELEASE_NAME}/config/local-ide.example.toml" \
  127.0.0.1:7878 http://127.0.0.1:7878 127.0.0.1 /tmp/local-ide "/release/${RELEASE_NAME}/web" http://127.0.0.1:9/v1 \
  >"${work_dir}/config.toml"

# Files go in on stdin because the Docker VM may not share the repository directory
COPYFILE_DISABLE=1 tar --no-mac-metadata --no-xattrs -cf - -C "$work_dir" . |
  docker run --rm -i --platform linux/amd64 --network none "$image" bash -euc '
    mkdir /release
    tar -xf - -C /release --no-same-owner
    chmod -R a+rX /release
    useradd --system --no-create-home --shell /usr/sbin/nologin engine
    release=/release/'"${RELEASE_NAME}"'
    setpriv --reuid engine --regid engine --clear-groups env LLAMA_SWAP_KEY=test \
      "$release/local-ide" --config /release/config.toml &
    for attempt in $(seq 1 60); do
      if (exec 3<>/dev/tcp/127.0.0.1/7878) 2>/dev/null; then break; fi
      sleep 0.5
    done
    exec 3<>/dev/tcp/127.0.0.1/7878
    printf "GET /api/health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n" >&3
    response=$(timeout 10 cat <&3)
    printf "%s\n" "$response"
    printf "%s" "$response" | tail -n 1 | grep -qx ok
    if command -v systemd-analyze >/dev/null; then
      systemd-analyze verify "$release/systemd/local-ide.service"
    else
      echo "systemd-analyze is absent in the image"
    fi
  '
printf 'pass: /api/health answers ok in %s as an unprivileged user\n' "$image"
