#!/usr/bin/env bash
# Install muxal's icon + .desktop entry for the current user, so it shows up in
# the app launcher and its desktop notifications carry the muxal name + icon.
#
#   scripts/install-desktop.sh            # build release + install
#   scripts/install-desktop.sh --no-build # install using the existing binary
#   MUXAL_EXEC=/path/to/muxal …           # point Exec at an installed binary
#                                         # (implies --no-build; used by
#                                         #  install.sh / promote.sh flows)
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin="${MUXAL_EXEC:-$repo_root/target/release/muxal}"

# rustup keeps cargo in ~/.cargo/bin, which is not on PATH in every shell.
cargo_bin="$(command -v cargo || true)"
cargo_bin="${cargo_bin:-$HOME/.cargo/bin/cargo}"

if [[ "${1:-}" != "--no-build" && -z "${MUXAL_EXEC:-}" ]]; then
    if [ ! -x "$cargo_bin" ]; then
        echo "install-desktop.sh: cargo not found (install rustup: https://rustup.rs)" >&2
        exit 1
    fi
    echo "building release binary…" >&2
    (cd "$repo_root" && "$cargo_bin" build --release -p muxal)
fi
if [[ ! -x "$bin" ]]; then
    echo "error: $bin not found (build first, or drop --no-build)" >&2
    exit 1
fi

data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
icon_dir="$data_home/icons/hicolor/scalable/apps"
apps_dir="$data_home/applications"
mkdir -p "$icon_dir" "$apps_dir"

install -m644 "$repo_root/crates/muxal/assets/muxal.svg" "$icon_dir/muxal.svg"

# Point Exec at the built binary's absolute path so it runs without PATH setup.
sed "s|^Exec=muxal\$|Exec=$bin|" "$repo_root/packaging/muxal.desktop" \
    > "$apps_dir/muxal.desktop"
chmod 644 "$apps_dir/muxal.desktop"

# Refresh the caches (best-effort; harmless if the tools are absent).
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps_dir" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 \
    && gtk-update-icon-cache -f -t "$data_home/icons/hicolor" >/dev/null 2>&1 || true

echo "installed:" >&2
echo "  $icon_dir/muxal.svg" >&2
echo "  $apps_dir/muxal.desktop (Exec=$bin)" >&2
echo "muxal should now appear in your launcher; notifications will show its icon." >&2
