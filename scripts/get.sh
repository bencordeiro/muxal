#!/usr/bin/env bash
# Install or update muxal from the latest GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/bencordeiro/muxal/master/scripts/get.sh | sh
#
# or download this file, read it, and run it. Options:
#
#   --check      report what would happen; change nothing
#   --force      allow downgrades and same-version re-installs
#   --system     install system-wide (/usr/bin) instead of ~/.local/bin
#   --appimage   use the AppImage asset (Linux) instead of the native package
#   --help       this text
#
# Releases ship SHA256SUMS.txt and this script verifies the download against it
# before anything is installed; an unverified or mismatched download is refused.
# The swap is copy-then-rename (safe while an old muxal is running) and the
# previous binary is kept next to the new one as `muxal.bak`.
#
# Testing overrides (never needed in normal use):
#   MUXAL_LATEST_API         API URL returning {"tag_name":"vX.Y.Z"}
#   MUXAL_RELEASE_BASE_URL   base for $BASE/vX.Y.Z/<asset> downloads
#   MUXAL_BIN_DIR            install dir for the binary channels
#   MUXAL_CHANNEL            tar|deb|rpm|appimage|macos — skip detection
#   MUXAL_FAKE_OS / MUXAL_FAKE_ARCH   pretend platform
set -euo pipefail

API_URL="${MUXAL_LATEST_API:-https://api.github.com/repos/bencordeiro/muxal/releases/latest}"
BASE_URL="${MUXAL_RELEASE_BASE_URL:-https://github.com/bencordeiro/muxal/releases/download}"

FORCE=0 CHECK=0 SYSTEM=0 WANT_APPIMAGE=0
for arg in "$@"; do
    case "$arg" in
        --check) CHECK=1 ;;
        --force) FORCE=1 ;;
        --system) SYSTEM=1 ;;
        --appimage) WANT_APPIMAGE=1 ;;
        --help | -h)
            sed -n '2,27p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "get.sh: unknown option: $arg" >&2
            exit 2
            ;;
    esac
done

log() { printf '%s\n' "$*"; }
fail() {
    printf 'get.sh: %s\n' "$*" >&2
    exit 1
}
fetch() { curl -fsSL "$1" -o "$2"; }
run_root() {
    if [ "$(id -u)" = 0 ]; then "$@"
    elif command -v sudo >/dev/null 2>&1; then sudo "$@"
    else fail "this step needs root — rerun with sudo, or use --appimage (or the tar channel) for a per-user install"
    fi
}
# Atomic swap + keep the previous binary as .bak (rename, so a running muxal
# keeps its old inode; copying over a running binary fails with ETXTBSY).
swap_in() {
    local src="$1" dst="$2" tmp="$2.get-tmp.$$"
    [ -e "$dst" ] && cp -f "$dst" "$dst.bak"
    cp "$src" "$tmp"
    chmod +x "$tmp"
    mv -f "$tmp" "$dst"
}
version_lt() { # "1.2.3" < "1.2.10"
    [ "$1" != "$2" ] && [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -1)" = "$1" ]
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# ---- what is the latest release, and what is installed? ---------------------
fetch "$API_URL" "$tmp/latest.json" || fail "can't reach $API_URL — check your connection"
latest="$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp/latest.json" | head -1)"
[ -n "$latest" ] || fail "no release tag in the API response"
ver="${latest#v}"

current=""
if bin="$(command -v muxal)" && [ -n "$bin" ]; then
    # Binaries before v0.4.4 don't know --version and boot the full GUI
    # instead. Give the answer two seconds, then kill it and treat the version
    # as unknown — never block, never leave a stray window.
    verfile="$tmp/installed-version"
    "$bin" --version >"$verfile" 2>/dev/null &
    verpid=$!
    for _ in $(seq 1 10); do
        [ -s "$verfile" ] && break
        kill -0 "$verpid" 2>/dev/null || break
        sleep 0.2
    done
    kill "$verpid" 2>/dev/null || true
    wait "$verpid" 2>/dev/null || true
    current="$(awk '{print $2}' "$verfile" 2>/dev/null)"
fi

if [ -n "$current" ]; then
    if [ "$current" = "$ver" ] && [ "$FORCE" = 0 ]; then
        log "muxal $current is already up to date (latest is $latest)."
        exit 0
    fi
    if version_lt "$ver" "$current" && [ "$FORCE" = 0 ]; then
        fail "installed $current is newer than $latest — pass --force to downgrade"
    fi
    log "updating: $current -> $ver"
else
    log "installing muxal $ver"
fi

# ---- platform + channel ----------------------------------------------------
os="${MUXAL_FAKE_OS:-$(uname -s)}"
arch="${MUXAL_FAKE_ARCH:-$(uname -m)}"
case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) fail "unsupported architecture: $arch" ;;
esac

channel="${MUXAL_CHANNEL:-}"
if [ -z "$channel" ]; then
    if [ "$os" = "Darwin" ]; then channel=macos
    elif [ "$WANT_APPIMAGE" = 1 ]; then channel=appimage
    elif command -v dpkg >/dev/null 2>&1; then channel=deb
    elif command -v rpm >/dev/null 2>&1; then channel=rpm
    else channel=tar
    fi
fi

case "$channel" in
    deb)
        if [ "$arch" = aarch64 ]; then asset="muxal_${ver}-1_arm64.deb"
        else asset="muxal_${ver}-1_amd64.deb"
        fi ;;
    rpm)
        if [ "$arch" = aarch64 ]; then asset="muxal-${ver}-1.aarch64.rpm"
        else asset="muxal-${ver}-1.x86_64.rpm"
        fi ;;
    tar) asset="muxal-linux-${arch}.tar.gz" ;;
    appimage) asset="muxal-linux-${arch}.AppImage" ;;
    macos) asset="muxal-macos-universal.zip" ;;
    *) fail "unknown channel: $channel" ;;
esac
log "channel: $channel — asset: $asset"
[ "$CHECK" = 1 ] && {
    log "check only — nothing changed."
    exit 0
}

# ---- download + verify -----------------------------------------------------
url="$BASE_URL/$latest/$asset"
fetch "$url" "$tmp/$asset" || fail "download failed: $url"
fetch "$BASE_URL/$latest/SHA256SUMS.txt" "$tmp/SHA256SUMS.txt" ||
    fail "SHA256SUMS.txt missing for $latest — refusing to install unverified files"
want="$(awk -v a="$asset" '$2 == a {print $1}' "$tmp/SHA256SUMS.txt")"
[ -n "$want" ] || fail "no checksum for $asset in SHA256SUMS.txt"
got="$(sha256sum "$tmp/$asset" | awk '{print $1}')"
[ "$got" = "$want" ] || fail "checksum mismatch for $asset — refusing to install"

# ---- install ---------------------------------------------------------------
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
bin_dir="${MUXAL_BIN_DIR:-}"
if [ -z "$bin_dir" ]; then
    if [ "$SYSTEM" = 1 ]; then bin_dir=/usr/bin
    else bin_dir="$HOME/.local/bin"
    fi
fi

case "$channel" in
    deb)
        if command -v apt-get >/dev/null 2>&1; then run_root apt-get install -y "$tmp/$asset"
        else run_root dpkg -i "$tmp/$asset"
        fi ;;
    rpm)
        if command -v dnf >/dev/null 2>&1; then run_root dnf install -y "$tmp/$asset"
        else run_root rpm -U "$tmp/$asset"
        fi ;;
    tar)
        tar xzf "$tmp/$asset" -C "$tmp"
        d="$tmp/muxal-linux-$arch"
        [ -f "$d/muxal" ] || fail "unexpected tarball layout"
        if [ "$SYSTEM" = 1 ]; then
            run_root sh -c "mkdir -p '$bin_dir'"
            run_root sh -c "tmpdst='$bin_dir/muxal.get-tmp.\$\$'; cp '$d/muxal' \"\$tmpdst\"; chmod 755 \"\$tmpdst\"; mv -f \"\$tmpdst\" '$bin_dir/muxal'"
            run_root install -Dm644 "$d/muxal.desktop" /usr/share/applications/muxal.desktop
            run_root install -Dm644 "$d/muxal.svg" /usr/share/icons/hicolor/scalable/apps/muxal.svg
        else
            mkdir -p "$bin_dir" "$data_home/applications" "$data_home/icons/hicolor/scalable/apps"
            swap_in "$d/muxal" "$bin_dir/muxal"
            install -m 644 "$d/muxal.desktop" "$data_home/applications/muxal.desktop"
            install -m 644 "$d/muxal.svg" "$data_home/icons/hicolor/scalable/apps/muxal.svg"
            command -v update-desktop-database >/dev/null 2>&1 &&
                update-desktop-database "$data_home/applications" >/dev/null 2>&1 || true
            command -v gtk-update-icon-cache >/dev/null 2>&1 &&
                gtk-update-icon-cache -q -t "$data_home/icons/hicolor" >/dev/null 2>&1 || true
        fi ;;
    appimage)
        mkdir -p "$bin_dir"
        swap_in "$tmp/$asset" "$bin_dir/muxal" ;;
    macos)
        unzip -q "$tmp/$asset" -C "$tmp"
        [ -d "$tmp/muxal.app" ] || fail "unexpected zip layout"
        rm -rf /Applications/muxal.app
        cp -R "$tmp/muxal.app" /Applications/
        log "installed /Applications/muxal.app (ad-hoc signed: right-click → Open on first launch)" ;;
esac

log "done: ${current:-nothing} -> $ver ($channel). Previous binary kept as muxal.bak where applicable."
