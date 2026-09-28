# Dev / main workflow — project notes

Goal: **work on muxal while running muxal.** The installed daily driver
("main") and the tree under development stay strictly separated, with a
one-command bridge (`promote.sh`) between them, and a sandboxed dev instance
that can run *inside* the running main.

## The three ways a muxal binary runs

| Mode | Binary | Workspace (config/data) | Launcher entry | Use for |
| --- | --- | --- | --- | --- |
| **main** (official) | installed copy at `~/.local/bin/muxal` | real: `~/.config/muxal`, `~/.local/share/muxal` | yes | daily driving |
| **dev** (sandbox) | build tree, via `cargo run` inside `dev.sh` | sandbox: `.muxal-dev/config` + `.muxal-dev/data` | no | GUI-testing changes beside main |
| **ad hoc** | build tree, run in place | **real** workspace (unless you override `XDG_*`/`HOME` yourself) | no | quick one-off runs, debugging |

"Ad hoc" simply means running the freshly built binary straight from the build
tree without installing it: `cargo run -p muxal` (debug) or
`cargo build --release -p muxal && ./target/release/muxal`. No copy lands in
`~/.local/bin`, no `.desktop` entry is touched. Because it uses your **real**
workspace by default, it's for quick checks — not for parallel testing. For
parallel instances use **dev** (sandboxed), below.

## Separation & the two-instance rule

- The single-instance guard is **per workspace**: entering a workspace takes
  its lock. Two muxal processes run side by side happily on *different*
  workspaces (data dirs) but never clobber the same one.
- **main (real workspace) + dev (sandbox workspace) = different workspaces →
  they always coexist.** That is the supported "two instances" setup: keep
  driving main while a sandboxed dev GUI shows your uncommitted changes.
- Two runs against the *same* workspace (e.g. main + ad hoc) hit the lock —
  the second process won't double-open it.
- Extra sandboxes are free: `MUXAL_DEV_DIR=/tmp/a scripts/dev.sh`,
  `MUXAL_DEV_DIR=/tmp/b scripts/dev.sh`, … each is its own workspace.

## Daily loop (working on it, with it)

1. **Once:** `scripts/install.sh` — installs main to `~/.local/bin/muxal` and
   registers the launcher icon + `.desktop` entry.
2. **Use main** normally (real workspace, real agents).
3. **Develop inside a main pane:** `cd` to the repo, edit, run the gates
   (`cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, `cargo build -p muxal`).
4. **GUI-test the change:** `scripts/dev.sh` — boots a sandboxed muxal (run it
   in a pane of main if you like). Sandbox state persists in `.muxal-dev/`;
   delete the dir to reset first-run.
5. **Compare side by side:** leave main open and dev open at once — allowed,
   they're separate workspaces.
6. **Ship it to main:** `scripts/promote.sh` — release-builds the tree and
   atomically swaps `~/.local/bin/muxal`. Then **quit and relaunch main** to
   run the new binary (the running process keeps the old inode). The launcher
   entry needs no update; its Exec path is stable.
7. Repeat from 3.

## Why promote/install are safe while main is running

- Overwriting a *running* executable in place fails with `ETXTBSY`;
  `rename(2)` instead swaps the directory entry and leaves the running process
  on its old inode.
- Both scripts therefore: unlink `target/release/muxal` first (the historical
  launcher pointed there, so a running main may hold that file), build, copy to
  a temp file **in the install dir**, then `mv -f` onto `~/.local/bin/muxal`.
- Rule of thumb: never `cp` over the installed binary while main runs — always
  go through `promote.sh` / `install.sh` (rename semantics).

## Cheat sheet

| Task | Command |
| --- | --- |
| install main (once) | `scripts/install.sh` |
| promote dev → main | `scripts/promote.sh` |
| dev GUI (sandbox) | `scripts/dev.sh` / `scripts/dev.sh --release` / `scripts/dev.sh -- <muxal args>` |
| second sandbox | `MUXAL_DEV_DIR=/tmp/x scripts/dev.sh` |
| ad-hoc debug run (real workspace) | `cargo run -p muxal` |
| ad-hoc release run (real workspace) | `cargo build --release -p muxal && ./target/release/muxal` |
| launcher entry only | `scripts/install-desktop.sh` (`MUXAL_EXEC=/path` overrides Exec) |
| custom install dir | `MUXAL_BIN_DIR=/opt/bin scripts/install.sh` (same for promote) |

## Gotchas

- **Restart main after promoting** — the swap is a rename; the live process
  keeps the old binary until relaunched.
- The dev sandbox has its own first-run dialog, settings, and window geometry
  (under `.muxal-dev/`). On Linux, dev *panes* keep your real `HOME`, so agents
  inside them stay logged in; on macOS `dev.sh` sandboxes `HOME` too.
- Don't probe the GUI binary with CLI flags — `muxal --version` doesn't exist
  and will boot a full GUI against your real workspace.
- Release packages for other machines (`.deb` / `.rpm` / AppImage / `.tar.gz`)
  come from the release workflow; they install their own copy independent of
  this repo's `target/` tree.

## For agents working on this repo

Same loop as above, with two extra safety rules (also in `AGENTS.md`):

1. Gates → `scripts/dev.sh` GUI-test → `scripts/promote.sh` → tell the user to
   restart main. **Never kill/restart the user's running muxal yourself** —
   they may be working inside it; the promoted binary takes effect on their
   next restart.
2. Never overwrite `~/.local/bin/muxal` or `target/release/muxal` in place
   while any muxal runs (ETXTBSY), and never launch the GUI binary to "check"
   it (no CLI flags; boots against the real workspace). Use the isolated-`HOME`
   smoke recipe from `AGENTS.md` or `scripts/dev.sh`.
