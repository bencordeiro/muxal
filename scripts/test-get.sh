#!/usr/bin/env bash
# Sandbox tests for scripts/get.sh. Builds a fake release, serves it from a
# local HTTP server, and asserts install/update/refusal behavior inside a
# mktemp HOME — the real ~/.local/bin and system paths are never addressable.
#
#   scripts/test-get.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
get="$repo_root/scripts/get.sh"

s="$(mktemp -d)"
server_pid=""
cleanup() {
    [ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null || true
    rm -rf "$s"
}
trap cleanup EXIT

export HOME="$s/home"
mkdir -p "$HOME"
export MUXAL_BIN_DIR="$s/bin"
export MUXAL_CHANNEL=tar
export MUXAL_FAKE_OS=Linux
export MUXAL_FAKE_ARCH=x86_64
# Hermetic PATH: no real ~/.local/bin muxal may ever be found or launched.
export PATH="$MUXAL_BIN_DIR:/usr/bin:/bin"

# ---- fake release + API ----------------------------------------------------
serve="$s/serve"
mkdir -p "$serve/download/v9.9.9"

make_release() { # <version> — tarball with a stub `muxal` that reports it
    local v="$1"
    local stage="$s/stage-$v"
    rm -rf "$stage"
    mkdir -p "$stage/muxal-linux-x86_64" "$serve/download/v$v"
    printf '#!/bin/sh\n[ "$1" = "--version" ] && { echo "muxal %s"; exit 0; }\nexit 0\n' "$v" \
        >"$stage/muxal-linux-x86_64/muxal"
    chmod +x "$stage/muxal-linux-x86_64/muxal"
    : >"$stage/muxal-linux-x86_64/muxal.desktop"
    : >"$stage/muxal-linux-x86_64/muxal.svg"
    : >"$stage/muxal-linux-x86_64/LICENSE"
    tar czf "$serve/download/v$v/muxal-linux-x86_64.tar.gz" -C "$stage" muxal-linux-x86_64
    (cd "$serve/download/v$v" && sha256sum muxal-linux-x86_64.tar.gz >SHA256SUMS.txt)
    printf '{"tag_name":"v%s"}\n' "$v" >"$serve/latest.json"
}
make_release 9.9.9

port=$((20000 + RANDOM % 20000))
python3 -m http.server "$port" --directory "$serve" >/dev/null 2>&1 &
server_pid=$!
base="http://127.0.0.1:$port"
export MUXAL_LATEST_API="$base/latest.json"
export MUXAL_RELEASE_BASE_URL="$base/download"

for _ in $(seq 1 20); do
    curl -fsS "$base/latest.json" >/dev/null 2>&1 && break
    sleep 0.2
done
curl -fsS "$base/latest.json" >/dev/null 2>&1 || {
    echo "test-get: could not start the local server" >&2
    exit 1
}

# ---- harness ---------------------------------------------------------------
failed=0
ok() { echo "ok: $1"; }
bad() {
    echo "FAIL: $1"
    failed=1
}
installed() { "$MUXAL_BIN_DIR/muxal" --version 2>/dev/null | awk '{print $2}'; }

# 1. fresh install
if "$get" >"$s/out1" 2>&1 && [ -x "$MUXAL_BIN_DIR/muxal" ] && [ "$(installed)" = "9.9.9" ]; then
    ok "fresh install lands the binary reporting the release version"
else
    bad "fresh install ($( cat "$s/out1"))"
fi

# 2. idempotent: same version is a no-op
if "$get" >"$s/out2" 2>&1 && grep -qi "up to date" "$s/out2"; then
    ok "re-run with the same version reports up to date"
else
    bad "idempotent re-run ($( cat "$s/out2"))"
fi

# 3. --check changes nothing
rm -f "$MUXAL_BIN_DIR/muxal.bak"
if "$get" --check >"$s/out3" 2>&1 && [ ! -e "$MUXAL_BIN_DIR/muxal.bak" ]; then
    ok "--check reports without changing anything"
else
    bad "--check ($( cat "$s/out3"))"
fi

# 4. update: new release replaces the binary and keeps .bak
make_release 10.0.0
if "$get" >"$s/out4" 2>&1 && [ "$(installed)" = "10.0.0" ] && [ -e "$MUXAL_BIN_DIR/muxal.bak" ]; then
    ok "update swaps the binary and keeps muxal.bak"
else
    bad "update ($( cat "$s/out4"))"
fi

# 5. corrupt download is refused, binary untouched
cp "$MUXAL_BIN_DIR/muxal" "$s/before"
printf 'corrupt' >>"$serve/download/v10.0.0/muxal-linux-x86_64.tar.gz"
if "$get" --force >"$s/out5" 2>&1; then
    bad "corrupt download was accepted ($( cat "$s/out5"))"
elif cmp -s "$s/before" "$MUXAL_BIN_DIR/muxal"; then
    ok "corrupt download is refused and the binary is untouched"
else
    bad "corrupt download modified the installed binary"
fi
make_release 10.0.0

# 6. downgrade needs --force
make_release 9.9.9
if "$get" >"$s/out6" 2>&1; then
    bad "downgrade succeeded without --force ($( cat "$s/out6"))"
elif "$get" --force >"$s/out6b" 2>&1 && [ "$(installed)" = "9.9.9" ]; then
    ok "downgrade is refused unless --force"
else
    bad "--force downgrade ($( cat "$s/out6b"))"
fi
make_release 10.0.0
"$get" >/dev/null 2>&1

# 7. offline: unreachable API fails cleanly, nothing changes
cp "$MUXAL_BIN_DIR/muxal" "$s/before"
if MUXAL_LATEST_API="http://127.0.0.1:1/latest.json" "$get" >"$s/out7" 2>&1; then
    bad "offline run succeeded ($( cat "$s/out7"))"
elif cmp -s "$s/before" "$MUXAL_BIN_DIR/muxal"; then
    ok "offline fails cleanly and changes nothing"
else
    bad "offline run modified the installed binary"
fi

# 8. swap succeeds while the old binary is running (rename semantics)
cp /bin/sleep "$MUXAL_BIN_DIR/muxal"
"$MUXAL_BIN_DIR/muxal" 30 &
busy=$!
sleep 0.1
if "$get" --force >"$s/out8" 2>&1 && kill -0 "$busy" 2>/dev/null && [ "$(installed)" = "10.0.0" ]; then
    ok "swap succeeds while the old binary keeps running"
else
    bad "swap with running binary ($( cat "$s/out8"))"
fi
kill "$busy" 2>/dev/null || true

if [ "$failed" = 0 ]; then
    echo "all get.sh tests passed"
else
    echo "some get.sh tests FAILED" >&2
fi
exit "$failed"
