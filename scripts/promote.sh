#!/usr/bin/env bash
# Push the current dev tree over the installed "main" muxal:
# release-build, then atomically swap ~/.local/bin/muxal.
#
#   scripts/promote.sh
#   MUXAL_BIN_DIR=/opt/bin scripts/promote.sh   # match a custom install dir
#
# Safe to run from inside a running main (e.g. a muxal pane): the swap is a
# rename, so the running process keeps its old inode. Restart main (quit +
# relaunch) to pick up the new binary. Dev instances (scripts/dev.sh) and the
# dev sandbox are untouched. The launcher entry needs no update — its Exec
# path is stable.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest_dir="${MUXAL_BIN_DIR:-$HOME/.local/bin}"
dest="$dest_dir/muxal"

echo "building release binary…" >&2
# A running main may still hold target/release/muxal (the old launcher pointed
# there). Unlink it first — the running process keeps its inode — or rustc's
# write fails with ETXTBSY.
rm -f "$repo_root/target/release/muxal"
(cd "$repo_root" && cargo build --release -p muxal)

mkdir -p "$dest_dir"
# Copy-then-rename: see install.sh — in-place overwrite of a running binary
# fails with ETXTBSY, rename does not.
tmp="$(mktemp "$dest_dir/.muxal.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
cp "$repo_root/target/release/muxal" "$tmp"
chmod +x "$tmp"
mv -f "$tmp" "$dest"
trap - EXIT

echo "main updated: $dest — restart muxal to run it." >&2

# Clean up pre-rename (muxel) install leftovers so no launcher entry can point
# at a stale binary.
rm -f "$dest_dir/muxel" "$HOME/.local/share/applications/muxel.desktop"
