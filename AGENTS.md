# AGENTS.md

## What muxal is

muxal is a GPUI-based multi-agent terminal multiplexer (built on Zed's GPUI +
gpui-component): a tiled, tabbed workspace for running coding agents (Claude,
opencode, Amp, …) and shells side by side, with first-class git worktrees, agent
status tracking, and notifications. See `FEATURES.md` for the full feature list.

It's a native desktop GUI app (not a TUI) — each pane embeds a real terminal
emulator running a PTY child process.

## Architecture

Cargo workspace, five crates (depend downward only):

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
- `crates/muxal-tray` — platform tray backends and their data-only menu model;
  the app supplies localized labels and handles navigation/quit actions.

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
  The extension also reports blocking prompt lifecycle events in a separate
  `.json.input` runtime file, checked against both the bound session id and cwd.
  After changing the extension, run
  `node --experimental-strip-types scripts/test-pi-session-binding.mjs` as well.
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
- **Overlay focus** — all panes now use GPUI; the native web browser has been
  removed. Keep `MuxalApp::any_overlay_open` current for focus diagnostics and
  continue recording/restoring focus for input-holding popups.

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
- **Basic visual reviews are the user's job.** Do not control the desktop with
  mouse/keyboard automation or take screenshots for routine theme/layout checks.
  Run code/tests and the isolated launch smoke, then give the user a compact
  review checklist. Desktop control requires an explicit request for that work.

### Updating the installed main (agent checklist)

The user drives an installed **main** (`~/.local/bin/muxal`, real workspace) and
develops against a sandboxed **dev** (`scripts/dev.sh`, `.muxal-dev/`). When your
change should reach their local install:

1. Run the gates above, then GUI-test with `scripts/dev.sh` — sandboxed, never
   touches the real workspace, and coexists with the user's running main.
2. **Wait for the user to test dev and explicitly approve promotion.** Then
   `scripts/promote.sh` release-builds and atomically swaps
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

Always pass `--repo bencordeiro/muxal` to release/CI `gh` commands. With both
origin and upstream remotes, `gh` can otherwise infer upstream Muxel.

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

As of **v0.5.5** (tag `3fb4610`, published and promoted; verified 2026-10-20):

- **Music/keybindings implementation (2026-10-20):** authorized and implemented
  on `dev/music-keybindings`; see `docs/music-and-keybindings-plan.md`. No search
  or category filters: compact friendly rows, Modified only, individual resets,
  expandable numbered shortcuts/passthrough. Apply preserves unknown overrides;
  Reset/Cancel replace old live chords while preserving GPUI bindings. Settings
  now restores focus on close and rollback reinstalls the saved snapshot.
  Music is fully disabled by default: no worker/scan/device/timer while off.
  Settings → Music chooses an existing folder; sidebar controls above New Project
  provide Play/Pause and Skip, with volume and refresh in Settings. One player
  serves the process's windows, with bounded discovery and streaming MP3/FLAC/
  PCM WAV/Ogg Vorbis. Separate default-off autoplay starts once on project open;
  manual pause wins over later switches. No activity/focus sensor or automatic
  pause. Disable/quit releases resources. Main/dev processes remain independent.
  The local Linux gate passes 615 active tests, including generated four-format
  decode fixtures, bounded failure/queue tests, and silent native ALSA lifecycle.
  The isolated-HOME GUI smoke passes with no music worker/device while disabled.
  Rodio is playback-only with selected codecs; Linux adds ALSA, with t64/old
  alternatives in .deb and full audio license notices in native packages.
  Human layout/listening review is pending; macOS native playback needs validation
  before a release. The local .deb builds with all audio notices and resolving
  libraries. Final stripped Linux binary growth is about 1.56 MiB (1.84%), or
  425 KiB after gzip. The running review sandbox is `.muxal-dev/music-review`
  via `scripts/dev.sh`; the user controls its music choices.
  Main is unchanged; no promotion/version bump/release is
  authorized. Do not automate desktop review or play audible test music.
  Human review confirmed music works and found a long project-name overflow in
  Settings → Projects. Project rows now constrain and ellipsize their text, with
  the full name on hover; the editor retains its own column. Restart dev manually
  to load the fix; never interrupt a running review or main yourself.
  The user's next idea is subtle optional navigation/workflow sound effects;
  record/design separately after this review, not implemented in this change.
- **Release complete:** Spider maps and Phase 2, Rust/JS/TS/JSX/TSX/Python import
  connections, brighter Default Dark, Clay Dark, subtle Clay/Fahrenheit surface
  lifts, measured appearance pickers, Antigravity and Aider presets, and complete
  built-in web browser removal. Default Dark remains the fresh-install default;
  there are 20 themes. Workbench Dark was declined and removed.
- **Verification:** native Linux x86/ARM and universal macOS release builds passed.
  All ten downloaded package checksums are OK, zero failures. The published Linux
  binary reports 0.5.5; both macOS bundle versions and both .deb metadata/dependency
  lists were checked. The published Linux binary has no retired browser runtime
  stack. Corrected master CI is green. Formatting, strict Clippy, workspace tests,
  build, 644-key i18n check, and isolated-HOME smoke passed. A post-tag test-only
  correction (`78a97df`) separates Linux invalid-UTF-8 filename fixtures from
  portable Unix symlink tests; production code is identical to the release tag.
  The local Linux gate passes 600 active tests (one manual benchmark ignored).
- **Local main:** `scripts/promote.sh` atomically installed 0.5.5 and refreshed
  the launcher. The installed binary reports 0.5.5. The user must restart main
  themselves; never kill or restart it. This release and promotion were explicitly
  authorized following dev review. Future changes need fresh promotion approval.
  `master`, `dev/spider-map`, and the annotated tag are pushed. GitHub notes use
  Added/Changed/Fixed/Removed/Development and the full comparison link. Always
  use `--repo bencordeiro/muxal` with release/CI gh commands.
- **Spider:** complete; design notes in `docs/spider-map-plan.md`. A normal
  resizable/tabbed pane, disabled by default in Settings. Disabling stops work
  and preserves tabs as placeholders. Radial/tree layouts, Structure/Git overlays,
  physical text-line sizing, Find, Inspect, and worktree scopes are implemented.
  Expansion/collapse retains the wider map while readable; geometry and node
  budgets, not repository size/depth, decide crowded-subtree focus. Back/Backspace
  or double-clicking the focused center restores the previous expansion/camera.
  History is bounded to 32 views and survives detach/re-dock. Resize/live activity
  never changes focus automatically. Small .cargo/.github expansion regressions
  are covered; existing label suppression must not force unrelated subtree focus.
  The header root button works with View closed; its redundant single-checkout
  menu row is hidden. Crowding/readability acceptance belongs to the user.
- **Spider Phase 2:** independent default-off Recent activity and Code connections.
  Saved files glow cyan for 25 seconds, otherwise only their nearest visible
  containing folder glows; the project root never glows. Navigation/refresh
  preserves original timestamps. Additions/renames/removals trigger coalesced
  inventory refresh while Recent activity is enabled/visible: one-second quiet
  time, five-second burst deferral, minimum five seconds between scan starts.
  Changes during scans remain pending; failures require manual retry. Camera and
  expansions survive refresh. Ordinary edits update glow/import links without
  inventory scans; other metrics use manual Refresh. Syntax-based local imports
  support Rust, JavaScript, TypeScript, JSX/TSX, and Python only, with exact
  incoming/outgoing paths, honest coverage limits, and optional three-hop indirect
  links. No additional languages, compiler/LSP/interpreter processes, alias
  guessing, or agent hooks. Agent attribution is explicitly declined. Native
  observers are shared per visible checkout (at most four), event queues/latest
  graphs are bounded, activity reads no source contents, and observers stop when
  unused or disabled. No whole-checkout polling. Keep these resource limits.
- **Browser removal:** complete. Web panes/presets/settings, HTML preview,
  webview backends/helpers, browser-only assets, and 69 unused Cargo packages are
  removed. Deliberate web links use the system browser; local files including
  HTML source use the editor. The filesystem file browser stays. Pure
  `muxal-core::compat` decoders retire old browser data without losing unrelated
  panes/tabs/weights/settings; loops bound to removed presets are disabled for
  review. Loading old data never opens URLs. Linux no longer requires
  GTK/GLib/WebKit/JavaScriptCore/libsoup; .deb dependencies, AppImage packaging,
  CI, licensing, and docs are updated. CI packages retain glibc 2.35 baseline.
  Never restore the embedded browser or alter the historical `ios/` snapshot.
- **Harness presets:** Pi is always seeded even when absent from PATH; Settings
  shows missing tools and launch menus filter them. Antigravity (`agy`) and Aider
  (`aider`) use normal interactive defaults and editable model options. Seed
  version 19 merges additions once without replacing matching custom presets.
  **Copilot is explicitly declined; do not add or propose it.**
- **Themes:** palette audit in `docs/theme-review.md`. Clay Dark uses warm
  charcoal/ivory/terracotta inspired by Claude; Clay and Fahrenheit surfaces
  received the requested subtle brightness lift. Refreshed Default Dark uses
  internal registry name `Muxal Default Dark` because GPUI rejects duplicates;
  helpers preserve public/persisted Default Dark and fresh/unknown fallback.
  Theme/language pickers use measured Select lists, bounded by live viewport,
  and resync on Settings open. Basic visual reviews belong to the user: no
  desktop input automation or screenshots without an explicit request.
- **Manual handoffs:** complete since v0.5.4. Opt-in prepare/review/start buttons
  send full reviewed text into a fresh recipient in the same directory/worktree.
  Temporary transfer files are removed after capture and durable workspace save;
  cancellation, late files, and old orphans are cleaned up. Old receiving panes
  retain referenced files until closed. Logic: `muxal-core::handoff`, documents:
  `muxal-store::handoff`, UI: `app/handoffs.rs`. Never trigger or chain handoffs
  automatically. Compact `input` status and Pi prompt lifecycle binding also
  shipped in v0.5.4, alongside search/focus/query-reply fixes.
- **Parked/declined — do not re-propose unasked:** tmux survival layer, layout
  presets, cursor-flicker work, startup-delay audit, macOS extras, attention inbox,
  and cross-pane scrollback search. The user prefers compact sidebar status for
  questions/permission requests and controls versioning/release scope.
- **Upstream** (`ProjectHax/muxel`) is watched for ideas only (last checked
  2026-10: 77 commits across v0.2.5–v0.3.1); remote/Windows/iOS work is irrelevant
  to this local-only fork. Preserve Muxel provenance and `ios/`. Personal backlog
  is `IDEAS.md`, deliberately ignored and never staged.
