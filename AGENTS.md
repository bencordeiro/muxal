# AGENTS.md

## What muxal is

muxal is a GPUI-based multi-agent terminal multiplexer (built on Zed's GPUI +
gpui-component): a tiled, tabbed workspace for running coding agents (Claude,
opencode, Amp, …) and shells side by side, with first-class git worktrees, agent
status tracking, and notifications. See `FEATURES.md` for the full feature list.

It's a native desktop GUI app (not a TUI) — each pane embeds a real terminal
emulator running a PTY child process.

## Architecture

Cargo workspace, four crates (depend downward only):

- `crates/muxal-core` — the **pure** domain model: the pane layout tree
  (`PaneNode`, `pane.rs`), worktree naming, agent presets (`AgentPreset`), tmux
  arg helpers, and the persisted `Workspace` / `Project` / `Instance` / `Settings`
  types. No UI, no I/O — fully unit-tested. Most logic that *can* live here,
  should. The old `control` / `readaloud` / `stt` / `tts` / `audio` / `ssh` /
  `remote_ops` / `winshell` modules were removed — this fork is local-only, so
  no voice, outside-control, or remote code lives here anymore.
- `crates/muxal-store` — persistence: workspaces, `workspace.json`, and settings,
  loaded/saved under the platform config/data dirs (XDG on Linux).
- `crates/muxal-terminal` — the PTY child + `alacritty_terminal` emulator
  (`TerminalSession`, `session.rs`), its GPUI view + custom paint element
  (`TerminalView`/`TerminalElement`, `view.rs`/`element.rs`), terminal query
  replies (`replies.rs` — see the invariant below), and agent status detection
  (`AgentStatus`, `classify`).
- `crates/muxal` — the GPUI application (`app.rs`, the large `MuxalApp` entity):
  window, sidebar, toolbar, pane rendering, settings UI (`settings_view.rs`),
  editor (`editor.rs`), git/tmux side effects (`integrations.rs`), pi session
  binding (`session_binding.rs` + the bundled extension in `assets/pi/`), and
  embedded assets (icons, themes) wired up in `main.rs`. The old `stt` / `tts` /
  `update` / `control` / `secrets` modules were removed — no speech, in-app
  updater, outside-control, or keychain code lives here anymore.

`ios/` — a separate **Swift/SwiftUI iOS companion app**, intentionally kept
as-is. It is **not** a cargo crate — the `cargo` gate does not build it. It
re-implements a **historical** slice of `muxal-core`'s old *remote protocol*
(tmux session naming, `RemoteLayout` `.muxal/workspace.json`, `classify`/markers)
from the era when desktop muxal supported remote/SSH projects. Desktop muxal no
longer implements remote/SSH at all, so nothing on the Rust side speaks that
protocol any more — `ios/` is kept as a snapshot of it and deliberately does not
track the desktop code. See `ios/README.md`.

### Key concepts

- **Pane tree** — a project's layout is `Option<PaneNode>`: `Leaf(LeafData {
  tabs: Vec<Uuid>, active })` (a tab group) or `Split { direction, sizes,
  children }`. Mutations (`split`, `add_tab`, `move_tab_to`, `move_into_split`,
  `move_pane_beside`, `remove`, …) are pure functions in `pane.rs` with tests.
- **Pane sizes are relative weights** — `Split.sizes` are weights rendered as
  `w / sum`, not pixels. Two rules keep untouched panes at their exact size
  (both have regression tests in `pane.rs`; don't "simplify" them away):
  - **close** (`remove`): the removed child's weight folds into the **smallest
    surviving sibling** of its split (ties share it) — closing a pane never
    resizes a pane you didn't touch.
  - **move** (`move_tab_to` / `move_into_split` / `move_pane_beside`): the
    vanishing span is transferred to the drop target's branch *before* the
    removal (`transfer_vanishing_*`), and the removal must use the no-fold path
    (`remove_keep_span`) — folding too would allocate the span twice.
- **Instance vs terminal** — an `Instance` is persisted metadata (id, program,
  worktree, …); the live `TerminalView`/PTY is owned by the app, keyed by instance
  id in `MuxalApp.terminals`. Runtime-only state (e.g. `AgentStatus`) is **not**
  persisted.
- **Back-compat persistence** — every new field on a persisted struct gets
  `#[serde(default)]` so older `workspace.json` / settings still load. Bump
  `PRESET_SEED_VERSION` when adding a built-in preset so existing workspaces merge it.
- **Terminal query replies are answered twice, deliberately** — a program that
  queries the terminal (device attributes, colors, …) waits briefly, then reads
  keystrokes: a *late* reply is read as typing (an agent prompt fills with
  `0;2501;1c`). So `replies.rs` runs a second VTE parser on the PTY **reader
  thread** and answers every query whose reply doesn't need the grid the moment
  the bytes arrive; alacritty's listener answers only grid-dependent replies
  (cursor position, mode reports) and drops its own copies of the rest. Rules
  when touching this area: both parsers must share `session::term_config()`;
  replies must keep query order (`Replies` holds a reader reply behind an owed
  grid reply); and `replies::tests::replies_match_alacritty_byte_for_byte` is
  the conformance gate — extend it when adding query types.
- **pi session binding** — muxal launches pi with a bundled extension
  (`assets/pi/muxal-session-binding.ts` via `--extension`) that writes
  `{"session_id","cwd"}` to `$MUXAL_SESSION_BINDING_FILE`; `session_binding.rs`
  adopts it so restart/resume picks the same conversation. Validation is
  deliberately loose: **no session-file existence check** (pi only creates
  session files once they have content — empty `/new` sessions must survive
  restarts), but the bound cwd must match the instance's directory. Don't
  tighten this without re-testing the empty-session case.
- **GPUI patterns** — render builds an element tree with `div()…`; handlers use
  `cx.listener(|this, ev, window, cx| …)`; the terminal is a custom `Element`
  (manual layout/paint). Mouse `on_drag_move` fires for all listeners — guard with
  `ev.bounds.contains(&ev.event.position)`. Two more rules learned the hard way:
  - gpui **never blurs a dropped focus handle** — destructive UI actions must
    hand focus to a live element (`focus_instance` / `deselect_pane`), never
    `window.blur()`. Input-holding popups (search palette, find panel) record
    their focus return on open and restore it on close (`restore_popup_focus`)
    or a hidden input keeps swallowing keystrokes and hotkeys.
  - Drag must never also open: rows open on `.on_click`, not `.on_mouse_down`
    (gpui cancels the click once the drag threshold passes — verified in gpui's
    `elements/div.rs`).
- **Native webview overlay rule** — browser panes (`browser.rs`) are native child
  windows that draw ABOVE all gpui content. `MuxalApp::any_overlay_open` must list
  **every** modal/palette/menu/drag flag; when you add a new overlay, add its flag
  there or the webview will float over it.

## Building & testing a feature

cargo is installed via rustup (`~/.cargo/bin/cargo`).

Run this gate after any change, and fix everything before considering it done:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings   # warnings are errors
cargo test --workspace                                   # unit/integration tests
cargo build -p muxal                                     # the GUI binary
```

- **Where tests go** — pure logic in `muxal-core` (pane ops, worktree, presets,
  settings seeding) is unit-tested there; prefer extracting decision logic into
  pure, testable functions (e.g. `classify` in `view.rs`). `muxal-terminal` has
  integration-style tests that spawn a real PTY.
- **gpui `#[test]` gotcha** — in a file that does `use gpui::*`, `#[test]`
  resolves to gpui's attribute macro. In test modules, import only what you need
  (`use super::{Foo, bar}`) instead of `use super::*` so the built-in `#[test]` is
  used.
- **i18n (Tier 1 only)** — user-facing strings go through `t("…")`; after adding
  or changing any, run `python3 scripts/translate.py --extract-only` to sync the
  catalogs — CI's `i18n` job runs `--check` and fails on drift. Tier 2 (LLM
  regeneration of the non-English catalogs) is **explicitly declined** — don't
  run it, don't propose it.
- **Dev profile / `target/` hygiene** — `[profile.dev] debug = "line-tables-only"`
  keeps build/test binaries ~370 MB instead of ~1 GB (89% of the fat ones was
  DWARF); switch back to `debug = 2` only for in-debugger local-variable work.
  When `target/` grows: `rm -rf target/debug/incremental` is always safe (pure
  cache), and `cargo clean -p muxal -p muxal-core -p muxal-store -p
  muxal-terminal -p muxal-tray` reclaims our artifacts while keeping gpui's.

### Verifying GUI changes

Unit tests can't see the UI, so for anything visual:

- **Smoke test** (does it launch without panicking?) — run the binary against an
  isolated workspace for a few seconds; "ok" means it stayed up:

  ```sh
  s=$(mktemp -d)
  HOME="$s" XDG_CONFIG_HOME="$s/config" XDG_DATA_HOME="$s/data" \
    ./target/debug/muxal >/dev/null 2>&1 & pid=$!
  sleep 5
  if kill -0 $pid 2>/dev/null; then echo ok; kill $pid; else wait $pid; echo "exited early: $?"; fi
  rm -rf "$s"
  ```

  **Isolation must override `HOME`, not just the `XDG_*` vars.** The `directories`
  crate honours `XDG_*` only on Linux; on macOS it ignores them and uses
  `~/Library/Application Support/dev.muxal.muxal` — the real workspace, possibly
  with the user's own muxal running on it. It does follow `$HOME`, so setting both
  covers Linux and macOS. The script backgrounds and kills the process rather than
  using `timeout`, which macOS doesn't ship.

- **Interactive** — `scripts/dev.sh` runs muxal against an isolated sandbox
  (`.muxal-dev/`) so testing never touches the real workspace. A fresh workspace shows
  the first-run welcome dialog; accepted-terms / window geometry / layout live
  under the sandbox's data dir. Args go to cargo (`--release`); anything after `--`
  goes to muxal. On Linux it isolates with `XDG_*`; on macOS (where those are
  ignored) it builds first, then runs only the binary with `HOME` set to
  `.muxal-dev/home` — so agents in its panes start without your login/config.
- **Installed main vs dev** — `scripts/install.sh` installs the release binary to
  `~/.local/bin/muxal` (+ launcher entry); `scripts/promote.sh` release-builds and
  atomically swaps that binary (copy-then-rename, safe while main runs; restart
  main to switch). Never overwrite the installed binary with `cp` while main is
  running — rename only, or ETXTBSY. See README "Dev / main workflow".
- State each visual behavior that needs a human's eyes — the harness can build and
  smoke-test, but can't see colors/layout.

### Updating the installed main (agent checklist)

The user drives an installed **main** (`~/.local/bin/muxal`, real workspace) and
develops against a sandboxed **dev** (`scripts/dev.sh`, `.muxal-dev/`). When your
change should reach their local install:

1. Run the gates above, then GUI-test with `scripts/dev.sh` — sandboxed, never
   touches the real workspace, and coexists with the user's running main.
2. `scripts/promote.sh` — release-builds and atomically swaps
   `~/.local/bin/muxal` (copy-then-rename, safe while main runs).
3. **Never kill or restart the user's running muxal yourself** — they may be
   working inside it. Tell them the new binary takes effect on their next
   restart of main.
4. If `~/.local/bin/muxal` doesn't exist yet, `scripts/install.sh` instead
   (also registers the launcher icon + `.desktop` entry).

Hard rules while doing this:

- Never `cp`/overwrite `~/.local/bin/muxal` or `target/release/muxal` in place
  while any muxal is running (ETXTBSY); the scripts unlink/rename correctly.
- Never launch the installed or built GUI binary "to check" — `--version` is its
  only CLI flag; anything else boots a full GUI against the **real** workspace. Use the
  isolated-`HOME` smoke recipe above or `scripts/dev.sh`.
- Fresh sandbox when needed: `MUXAL_DEV_DIR=/tmp/x scripts/dev.sh`.
- Full notes: `docs/dev-main-workflow.md`; user-facing summary in README
  ("Dev / main workflow").

## Cutting a release

The user decides when and what to release (they GUI-test in dev first). The
ritual, in order:

1. **Bump the version in three places**: workspace `Cargo.toml` `version`,
   `packaging/macos/Info.plist` (`CFBundleVersion` +
   `CFBundleShortVersionString`), and `Cargo.lock` (cargo updates it on the next
   build — commit it).
2. Run the full gate; `./target/debug/muxal --version` must print the new
   version.
3. Commit `Release vX.Y.Z` — subject + body only, changelog prose with
   user-facing items first and dev-only items last — then an annotated `vX.Y.Z`
   tag and push `master` + the tag. CI builds every package from the tag.
4. Verify the published assets — don't let a `tail` mask a failure: run
   `sha256sum -c SHA256SUMS.txt` and count OK vs FAILED explicitly (all OK),
   and run the published Linux binary's `--version`.
5. `scripts/promote.sh`, then tell the user to restart main.

**Packaging note:** the `.deb` `depends` list in `crates/muxal/Cargo.toml` is
**hand-maintained** — cargo-deb's `$auto` resolves against the build host and
produced pre-`t64` package names that no longer install on Ubuntu 24.04+ /
Debian 13. Nothing regenerates it: when a native dependency changes, re-run
`dpkg-shlibdeps -O -e target/release/muxal` and update the list (use
`t64 | old` fallbacks for renamed packages).

## Features doc

`FEATURES.md` (repo root) is the canonical catalogue of user-facing features.
**When you add or change a user-facing feature, update `FEATURES.md` in the same
change** — add or edit its entry under the right category (sentence or two each;
add a new `##` category if none fits).

## Git commits

- Never put personal email, phone, home paths, API keys, or session URLs in commits.
- Do not append `Claude-Session:`, `Co-Authored-By:`, or other attribution trailers unless I explicitly ask.
- Before committing, run `git config user.email` and confirm it is the project's intended commit identity (not a personal address).
- Commit messages: subject + body only — no tooling metadata or session links.
- `IDEAS.md` is a personal idea scratchpad — deliberately **untracked and
  gitignored**; never `git add` it (an earlier `git add -A` slipped it into
  history and it had to be purged).

## Where things stand (update this section when it changes)

As of **v0.5.3** (tag `d085e84`, promoted to `~/.local/bin/muxal`):

- **Unreleased on `master`** since the tag: the `.deb` install fix for current
  Ubuntu/Debian (hand-maintained depends) and terminal query replies on the PTY
  reader thread. Both fully gated and verified — they ride along in the next
  release. Nothing else is in progress.
- **Passed on / parked — do not re-propose without being asked:** the tmux
  survival layer (tmux isn't even installed on this machine; the user isn't
  interested), layout presets, cursor-flicker work, startup-delay audit, and
  all macOS extras (app menu, notifications) — the user runs macOS and says it
  works fine.
- **Upstream** (`ProjectHax/muxel`) is watched for **ideas only** (last checked
  2026-10: 77 commits across v0.2.5–v0.3.1; its remote/Windows/iOS bulk is
  irrelevant to this local-only fork). Findings are distilled into `IDEAS.md`.
- Backlog lives in `IDEAS.md` (spider map `sketched`, attention inbox,
  cross-pane scrollback search, worktree board, day timeline, cost meter, …).
- The user drives versioning: patch releases when enough has landed (v0.5.2 =
  three changes, v0.5.3 = one fix), tested in `scripts/dev.sh` before shipping.
