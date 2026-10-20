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

As of **v0.5.4** (tag `d1a01a4`, published and promoted):

- **Release authorization (2026-10-19):** the user requested the complete dev
  work be released as **v0.5.5** after adding one or two common harness presets.
  Add **Antigravity (`agy`)** and **Aider (`aider`)**; Copilot is explicitly
  declined and must not be added or proposed again. Pi is already seeded even
  when absent from PATH (Settings shows missing tools; launch menus filter them).
  The user has authorized this release and its normal promotion ritual following
  their dev reviews. Earlier "promotion unapproved" notes below describe the
  previous state; do not ask again. Master now includes the completed dev work;
  the app and macOS bundle versions are bumped to 0.5.5. The final gate passed:
  formatting, strict Clippy, 599 active Rust tests (one manual benchmark ignored),
  build, 644-key i18n check, and isolated-HOME smoke. The built binary reports
  0.5.5. Tagged release, published-asset verification, and atomic promotion remain.
  Never restart main.

- **Release CI fixture follow-up:** macOS rejects the invalid UTF-8 filename
  deliberately created by one Spider scanner test. Split its Linux-only
  filesystem case from portable Unix symlink coverage; no production code
  changed. The local gate passes with 600 active tests (one benchmark ignored),
  build, i18n check, and isolated-HOME smoke. Corrected master CI is required
  before promotion; the already-pushed release tag stays unchanged.

- **Release content:** manual agent handoffs, the compact sidebar **input** label
  and Pi prompt lifecycle binding, terminal search ownership/stale-match fixes,
  immediate history-clear repaint, live focus after pane close/detach, timely
  terminal query replies, and the `.deb` dependency fix for current Ubuntu/Debian.
  Handoffs and bug fixes are fully gated; all 548 Rust tests pass. The user
  verified handoffs in dev, including full-text delivery and transfer-file cleanup.
- **Release verified:** `master` and annotated `v0.5.4` are pushed; all native
  release builds and regular CI checks passed. All ten published package checksums
  are OK (zero failures). The downloaded Linux binary reports v0.5.4; macOS bundle
  versions and the `.deb` t64 alternatives were checked too. GitHub release notes
  include Added, Fixed, Development, and the full comparison link.
- **Local main:** promoted using `scripts/promote.sh`; the installed binary
  reports v0.5.4. The user's running main still needs their own restart to switch;
  never restart or kill it yourself. Spider, including Phase 2, is ready
  for user review on `dev/spider-map`. Promotion has not been approved.
- **Theme review (dev):** all 19 existing choices reviewed in
  `docs/theme-review.md`. Added optional **Clay Dark** (Claude-inspired warm
  charcoal, ivory, terracotta); the public collection now has 20 themes. Default
  Dark remains the default. Clay Dark and Fahrenheit received the user-requested
  subtle lift of dark surfaces (about 2% toward white), with accents/text intact.
  The user declined Workbench Dark as too similar to existing choices; it is
  removed. Clay uses the existing loader/tint pipeline. The user liked Clay
  before the final small brightness lift; final brightness review remains.
  Existing review windows use older binaries until the user restarts dev. Basic
  visual reviews are user-led; do not automate desktop interactions for them.
  Promotion remains unapproved.
- **Appearance picker fix (dev):** the user reported the theme dropdown collapsing
  to a thin strip. Theme and language now use GPUI's measured `Select` lists
  rather than overflow-only `PopupMenu` sizing. Heights are capped by the live
  viewport, widths respect the settings content, and selections resync when
  Settings opens (including after Cancel). Full gate passed: formatting, strict
  Clippy, 597 active Rust tests, build, 644-key i18n check, and isolated-HOME
  launch smoke. User visual review remains pending; restart dev to use the fix.
- **Built-in web browser removal (dev):** implemented and locally verified.
  Formatting, strict Clippy, all 597 active Rust tests (one manual benchmark
  ignored), debug/release builds, extract-only i18n (644 keys), and the isolated
  HOME launch smoke passed. Browser panes/presets,
  settings/type/homepage controls, HTML preview, native webviews/helper process,
  browser-specific focus telemetry, and browser-only dependencies/assets are
  removed. Deliberate web-link actions use the OS handler; local files, including
  HTML source, use the editor. The filesystem file browser stays. Pure
  compatibility decoders in `muxal-core::compat` retire saved browser data while
  preserving other panes, tab groups, split weights, and settings; loops bound
  to retired presets are disabled for review. The native dependency audit
  (`ldd`, `readelf`, `dpkg-shlibdeps`) confirms no GTK/GLib/WebKit/JavaScriptCore/
  libsoup runtime requirement. The .deb list and CI build dependencies are
  updated; AppImage packaging no longer needs the browser exclusion closure.
  A local .deb builds with the expected five runtime dependencies. The local
  host requires glibc 2.39; published packages keep CI's Ubuntu 22.04 baseline
  (2.35). Native CI packages will be checked as part of the release ritual.
  Never restore the embedded browser
  or change `ios/`, which remains its historical snapshot. User dev review and
  explicit promotion approval are still required.
- **Passed on / parked — do not re-propose without being asked:** the tmux
  survival layer (tmux isn't even installed on this machine; the user isn't
  interested), layout presets, cursor-flicker work, startup-delay audit, and
  all macOS extras (app menu, notifications) — the user runs macOS and says it
  works fine. The attention inbox is also declined; the user prefers the compact
  sidebar status for requests needing a response.
- **Upstream** (`ProjectHax/muxel`) is watched for **ideas only** (last checked
  2026-10: 77 commits across v0.2.5–v0.3.1; its remote/Windows/iOS bulk is
  irrelevant to this local-only fork). Findings are distilled into `IDEAS.md`.
- **Manual agent handoffs** are complete in v0.5.4 (developed on
  `dev/agent-handoffs`):
  opt-in prepare/review/start buttons (Settings → Behavior), full reviewed text
  sent directly in a fresh recipient's opening prompt, same source directory/
  worktree, and one-shot input. Source transfer files are temporary: after capture
  and a durable save, the review lives in workspace.json and transfers are deleted.
  Cleanup also handles cancellation, late files, and earlier orphaned transfers;
  old receiving panes retain referenced files until closed. Handoff logic is in
  `muxal-core::handoff`, documents in `muxal-store::handoff`, UI in
  `app/handoffs.rs`. Never trigger or chain handoffs automatically. **Complete:**
  the user verified direct-text delivery in dev and cleanup was confirmed for
  their exact test request; all 548 Rust tests, real Pi input verification, and
  sandbox launch gates passed. **Future changes still require explicit user
  approval before promotion.**
- **Next feature direction:** the user authorized implementation of the agreed
  Spider map after v0.5.4. The proposal and open decisions live in
  `docs/spider-map-plan.md`. The user confirmed a normal resizable/tabbed pane
  beside agents, with compact map controls and both layout/overlay choices added
  gradually. Spider must be opt-in in Settings and **disabled by default**;
  disabling stops its background work and preserves existing tabs as placeholders.
  The initial live sandbox render received positive preliminary feedback. The
  full gate passes (597 Rust tests, plus the manual observer benchmark),
  extract-only i18n is current (644 English keys), and the
  isolated-HOME launch smoke passed. A restored disabled tab launched no scans;
  the enabled native map scanned this repository in about 0.3 seconds. The
  100,000-file pure-model fixture completed in about 0.8 seconds with 53 MiB peak
  RSS and at most 300 visible nodes. Final human review is pending for compact
  readability, controls/navigation, settings disable/re-enable, and pane/window
  operations. **Phase 2 is implemented in dev:** independent default-off Recent
  activity (25-second cyan glow) and Code connections (Rust, JavaScript,
  TypeScript/JSX/TSX, and Python imports, optional indirect paths up to three hops).
  Related paths are revealed within the existing visible-node budget; Inspect
  supplies exact incoming/outgoing paths and coverage limitations. This uses
  the existing syntax parser, without compiler/LSP processes or agent hooks.
  Native events are shared per visible checkout (at most four); the activity
  observer reads zero source contents, never polls the checkout, and observation
  stops when overlays are off or no map is visible. Bounded event queues retain
  only one latest graph. Atomic saves do not invent new permanent paths; source
  edits update links without rescanning. With Recent activity visible/enabled,
  detected additions, renames, and removals now trigger coalesced background
  inventory refreshes (one-second quiet time, five-second burst deferral, minimum
  five seconds between scan starts). Changes during a scan stay pending; failure
  requires manual retry; hiding/disabling the final activity consumer cancels
  automatic work. Camera, expansions, and glow ages survive refresh. The header
  checkout label now returns to the root overview independently of View; Back
  restores the previous view. Native sandbox checks confirmed automatic create,
  rename, and removal refreshes, a regular save without an inventory scan, and
  the header button working with View closed. Metrics otherwise use manual
  Refresh. Measured
  observer startup on this repository: about 20 ms for activity only; about
  281 ms for connections, 81 source/manifest reads, 75 source files and 145 links
  after adding JS/TS/Python (6 unresolved imports, no partial-coverage flag).
  Agent checks confirmed the visible glow/fade, direct/indirect inspector links,
  and observer shutdown when toggles are off or the shell is maximized. Final
  visual acceptance remains the user's. File activity never identifies an
  agent; the user has declined agent attribution entirely. Keep
  these resource limits and honest labels. Do not promote without explicit approval.
  Human review clarified **expansion first**: double-click/Enter/Space opens or
  closes a branch within the wider map while readable, previewing immediate
  child folders. Crowding is decided by fitted node/label geometry and the pane's
  node budget, not total repository size or depth alone. Only crowded expansion
  focuses a subtree; double-clicking its center or using Back/Backspace restores
  the previous expansion and camera. History is bounded to 32 views and survives
  detach/re-dock; resize/live activity never changes focus automatically. The
  recent-file halo now appears on the actual visible changed file, otherwise
  only its nearest visible containing folder. The project root never receives
  this activity halo; navigating does not restart its 25-second clock. Projection
  is cached, so pan/hover does not rescan activity paths. Native sample checks
  confirmed successive nested expansions preserving the whole small project,
  double-click collapse, file/folder glow transfer, and a quiet root. Final human
  review should judge the crowding cutoff in the user's preferred pane sizes.
  Follow-up fixed small-folder focus in broad checkouts: expansion now compares
  label suppression with the previous view, so unrelated existing label crowding
  does not force `.cargo` into focus. Node collisions, clipping, and node budgets
  remain hard limits. Regression coverage opens `.cargo` and `.github` in both
  orders at three pane sizes, checks collapse, and preserves crowded-branch focus.
  View hides its redundant single-choice Checkout row; worktree selection and
  missing-checkout recovery retain it. The review sandbox is **Spider — folder
  expansion fixes**; prior running windows retain their older builds. The user
  approved committing and pushing the current Spider checkpoint on its dev
  branch; additional user-requested work remains before any release. No version
  bump, release tag, merge to master, or promotion is authorized by that approval.
- Cross-pane scrollback search is also declined; don't re-propose it unasked.
- Backlog lives in `IDEAS.md` (spider map `sketched`, worktree board,
  day timeline, cost meter, …).
- The user drives versioning: patch releases when enough has landed (v0.5.2 =
  three changes, v0.5.3 = one fix), tested in `scripts/dev.sh` before shipping.

- **Spider follow-up language boundary (2026-10-19):** the user authorized basic
  code connections for JavaScript, TypeScript (including JSX/TSX), and Python,
  alongside existing Rust support. No additional languages are wanted. Reuse
  bundled syntax parsers and the shared bounded observer; do not add language
  servers, compiler/interpreter execution, custom-alias guessing, or agent hooks.
  Agent attribution is explicitly declined. The language gate passed with 607
  tests; native checks showed the expected JS/TS and Python links and paths.
- **Default Dark refresh (2026-10-19):** the user requested a brighter, balanced
  charcoal theme while retaining Default Dark as the fresh-install default.
  The new bundled palette has layered charcoal surfaces, soft text, pale-blue
  accents, and distinct terminal/syntax/status colors. GPUI rejects duplicate
  theme names, so the registry uses `Muxal Default Dark` internally; application
  theme helpers map the existing Default Dark label and empty settings to it.
  Keep the picker label, persisted choices, and fresh/unknown-theme fallback
  coherent. Other saved theme choices stay selected. Full gate passed with
  608 tests, strict Clippy, build, 659-string i18n extraction/check, and an
  isolated-HOME smoke with saved Default Dark. Native checks confirmed fresh
  fallback, the public picker label, switching Light → Dark, saving Default Dark,
  menus/controls, ANSI colors, and JS/TS/Python map links. Core text contrast is
  about 12.1:1 and muted text 6.9:1 on the main background. The current sandbox
  is **Muxal — Default Dark review**. Final visual acceptance remains the user's;
  more user tasks remain before any release or promotion.
