#!/usr/bin/env bash
# One-time install of muxal as this user's "main" app:
#   1. release-build the binary
#   2. install it to ~/.local/bin/muxal (atomic swap; safe while main runs)
#   3. register the launcher icon + .desktop entry pointing at that path
#
#   scripts/install.sh
#   MUXAL_BIN_DIR=/opt/bin scripts/install.sh   # override the install dir
#
# Day-to-day afterwards:
#   scripts/dev.sh      # isolated dev instance (safe inside a main pane)
#   scripts/promote.sh  # push the dev tree over the installed main
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest_dir="${MUXAL_BIN_DIR:-$HOME/.local/bin}"
dest="$dest_dir/muxal"

# rustup keeps cargo in ~/.cargo/bin, which is not on PATH in every shell —
# resolve it explicitly so the script works wherever it's run from.
cargo_bin="$(command -v cargo || true)"
cargo_bin="${cargo_bin:-$HOME/.cargo/bin/cargo}"
if [ ! -x "$cargo_bin" ]; then
    echo "install.sh: cargo not found (install rustup: https://rustup.rs)" >&2
    exit 1
fi

echo "building release binary…" >&2
# A running main may still hold target/release/muxal (the old launcher pointed
# there). Unlink it first — the running process keeps its inode — or rustc's
# write fails with ETXTBSY.
rm -f "$repo_root/target/release/muxal"
(cd "$repo_root" && "$cargo_bin" build --release -p muxal)
mkdir -p "$dest_dir"
# Copy-then-rename: overwriting a *running* executable in place fails with
# ETXTBSY, but a rename swaps the directory entry and leaves the running
# process on its old inode.
tmp="$(mktemp "$dest_dir/.muxal.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
cp "$repo_root/target/release/muxal" "$tmp"
chmod +x "$tmp"
mv -f "$tmp" "$dest"
trap - EXIT
echo "installed binary: $dest" >&2

case ":$PATH:" in
*":$dest_dir:"*) ;;
*) echo "note: $dest_dir is not on your PATH — add it to run 'muxal' by name." >&2 ;;
esac

MUXAL_EXEC="$dest" "$repo_root/scripts/install-desktop.sh" --no-build

# Clean up pre-rename (muxel) install leftovers so no launcher entry can point
# at a stale binary.
rm -f "$dest_dir/muxel" "$HOME/.local/share/applications/muxel.desktop"
