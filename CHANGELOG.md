# Changelog

User-facing changes and fixes are recorded here as they land. GitHub releases
publish the matching version section; older releases remain available on the
[releases page](https://github.com/bencordeiro/muxal/releases).

## Unreleased

### Added

- Flight Deck theme: black-glass terminals in a steel-grey instrument panel,
  with magenta focus and cyan, amber, and green for working, input, and done.
  It replaces Ember Observatory; a saved Ember Observatory selection switches to
  Flight Deck.
- Theme picker swatches: each theme is listed with a miniature of its chrome,
  terminal, focused header, accent, and status colors.
- Pane headers show `±N` beside the diff button when that pane's checkout has
  uncommitted files.
- Peek on hover: resting the pointer on an agent's sidebar row shows a live
  tile of that agent's terminal beside the sidebar, without switching to it.
- Overview: the dashboard is now a live grid of every agent across projects.
  Each tile shows status, age, and the bottom rows of that agent's terminal,
  refreshed every second; click one to jump to its pane.
- Status on tabs: a tab's agent icon takes its status color, and the shown
  tab's underline turns amber when the agent needs input and green when it is
  done. Colors adjust automatically when they would blend into a themed header.

### Fixed

- Molokai Dark: secondary labels were too dark to read and the sidebar matched
  the background exactly. Both are corrected; its colors are otherwise unchanged.
- Gruvbox Dark, Gruvbox Light, and Spaceduck showed working and input in the
  same color. Each now has three distinct status colors from its own palette.

### Changed

- Pane header icons are larger and fill the tab strip's existing height; the
  strip itself is no taller. Tabs may be wider, so a single tab shows its full
  title. The diff pane's refresh control is a proper icon.
- Regroup the top bar by purpose, with hairlines between groups: the tmux and
  worktree options sit beside the launch button they affect; Run task, Loops,
  and Snippets follow; then Restart and the pinned projects. Git diff moves to
  the right beside the dashboard. The duplicate Close pane button is removed
  (each pane's header has its own).
- Give Restart, Snippets, the workspace switcher, and project memory their own
  icons instead of sharing one with another control.
- Sidebar project rows keep the project name visible: a long branch name
  truncates first, and the full name and branch show on hover. Memory, pin, and
  file-browser buttons appear on the active project and the hovered row.
- Attention fades with age: working and input always show in full color; a
  finished turn does for a day, then rests as plain muted text, and after three
  days reads `stale` like an idle pane. The same rule applies to the sidebar,
  tabs, and the overview.
- A working badge shows how long the agent has been working once it passes a
  minute (`working · 40m`).
- Shell panes no longer repeat their project's path: at the project root they
  show `Shell` (numbered when there are several), in a subfolder the relative
  path (`./server`). Path titles keep their last folder visible when truncated.
- The Notifications sidebar section is hidden while empty.
- Pane header buttons sit in spaced groups, with Close set apart from maximize
  and pop out.

## v0.5.8 — 2026-11-09

### Fixed

- Release a saturated closed terminal's pending PTY data after forced child
  termination, so native BSD/macOS writers can leave their output wait and be
  reaped even while a closed session is retained. Live output is never flushed.

### Development

- Use the native Intel macOS runner for routine CI and release packaging after
  repeated hosted Apple Silicon allocation failures. The universal app retains
  both Intel and Apple Silicon architectures.
- Add test-only shutdown phase reporting and an on-demand native macOS terminal
  regression workflow for full-queue shutdown failures.
- Make the full-queue shutdown fixture deterministic: explicitly ignore hangup,
  wait for a full queue, and separately check immediate channel closure and
  eventual child reaping. The native regression and all 131 terminal tests pass
  after the runtime fix, as does the full 631-test native Mac workspace suite.

**Full Changelog**: https://github.com/bencordeiro/muxal/compare/v0.5.7...v0.5.8

## v0.5.7 — 2026-11-09

### Added

- Themes can now color the terminal, the gaps behind panes, pane headers, and
  status badges independently. Themes that don't use this look the same as before.
- Four themes: Verdigris (deep teal, bronze, and copper), Rosewood (red-brown wood
  and cool aqua), Ledger (warm paper with dark terminals), and Ledger Dark.

### Changed

- Manual handoffs now recognize Codex, Antigravity, Claude, OpenCode, and Pi
  started inside Shell panes on Linux and macOS. The new pane uses the agent's
  actual working directory and preserves known worktree identity. Muxal checks
  the foreground process and directory before sending and starting the handoff.
- Refine Mainframe's hacker-green look: phosphor text and accents stay, now on a
  black screen inside a neutral graphite frame with tiered syntax greens. Finished
  agents read cyan so they no longer match working agents.
- Refresh Clay Dark with deep warm ink terminals inside a lighter stone frame and
  a terracotta header on the focused pane.
- Rework Ember Observatory with a night-sky terminal, coal top bar, and ember
  focused header. Its working and input statuses were identical and now differ.
- Rework Ayu Dark with ink terminals in a slate-blue frame and a citron accent, and
  make its previously near-unreadable secondary text legible.
- Theme zoning changes colors only; existing layouts and unzoned theme fallbacks
  are preserved, and Ayu Light is unchanged.

### Fixed

- Keep worktrees and their registry entries when Git safety checks or removal
  fail. Automatic cleanup refuses dirty worktrees and uses non-forced branch
  deletion; commit/merge cleanup refuses newly written uncommitted files.
- Preserve word/line deletion for modified Backspace, including when the
  platform supplies a character for the key.
- Refuse terminal launch when its working directory is missing or inaccessible,
  avoiding a silent fallback to the home directory.
- Exclude recognizable Claude and Codex prompt drafts from status markers and
  recognize additional Claude permission forms without changing the compact
  input badge or Pi lifecycle binding.
- Respect Git excludes before appending the shared-memory directory to .gitignore.

### Performance

- Cache branch/program discovery against input metadata with a 60-second
  fallback, refresh worktree counts every ten seconds, and prevent overlapping
  worktree diff refreshes. Display Git reads avoid optional index writes.
- Bound each terminal output queue to four 64-KiB chunks, preserving output and
  exit order while open. Closing a pane wakes blocked senders and idle Unix
  readers; a closed pane's unreaped Unix child is force-killed after its grace
  period if it ignores hangup. Query replies still run before queue admission.

### Development

- Move macOS CI and release packaging to the supported macOS 15 runner.
- Added regression coverage for shell-agent identity and directory tracking,
  worktree failures, linked-worktree polling, draft-safe status parsing, bounded
  terminal streams and teardown, and theme-zone parsing and fallback behavior.
- The local Linux gate passes 638 active tests (one ignored), strict Clippy,
  formatting, build, the 705-key translation check, and isolated-HOME launch smoke.
- Terminal wakeups reuse the existing polling dependency; theme zones reuse
  serde. No additional native library dependency or browser runtime is introduced.

**Full Changelog**: https://github.com/bencordeiro/muxal/compare/v0.5.6...v0.5.7

## v0.5.6 — 2026-10-20

### Added

- **Optional local music player**, disabled by default. Enable it in Settings →
  Music and choose a folder. Compact controls above New Project provide Previous,
  Play/Pause, Next, and a toggle to repeat the current track.
- **Local audio formats:** MP3, FLAC, PCM WAV, and Ogg Vorbis. Nested folders play
  in relative-path order, and the queue wraps at either end. Files stay in place;
  no project files, copied library, streaming account, or microphone are needed.
- **Fine volume control:** a 0–100% slider with 1% steps and a percentage readout.
  Changes apply live and save when the slider is released; 0% mutes playback.
  Refresh folder reloads newly added music. Folder, volume, selected track, and
  repeat mode are remembered; ordinary startup stays paused.
- **Optional project-open autoplay**, independently off by default. It starts
  once when a project opens, including a restored project. Manual pause prevents
  later project switches from restarting music.

### Changed

- **Keybindings Settings** uses compact rows and friendly action names, with a
  Modified only toggle and individual shortcut resets. Numbered shortcuts and
  terminal passthrough expand separately to reduce scrolling. Existing shortcuts
  and terminal routing remain compatible.
- Music continues across projects, windows, focus changes, and minimize-to-tray.
  Previous/Next keep paused playback paused and still work while repeat is on.
  One player serves each app process; disabling music or quitting stops playback
  and releases audio resources.

### Fixed

- Cancel in Settings restores the original shortcuts and terminal passthrough
  instead of reapplying edited inputs. Reset and Cancel remove superseded live
  keybindings while preserving GPUI component shortcuts.
- Hidden shortcut rows retain drafts; unchanged defaults are not saved as custom
  overrides, unrelated saved action entries survive Apply, and invalid shortcut
  syntax is reported before applying.
- Settings returns focus to a live pane when closed. Switching Settings sections
  does not leave an invisible input capturing terminal typing.
- Long project names stay inside the project list column, truncate cleanly, and
  show their full name on hover without displacing the project editor.
- Music Settings Cancel restores the previous volume; reopening Settings resets
  the slider to the actual saved setting.

### Development

- Music discovery is bounded and cancellable, with explicit partial results and
  no filesystem watches or background library polling. Playback streams the
  current track. Disabled music creates no worker, scan, timer, or audio device.
- Invalid tracks and output-device failures use bounded handling rather than
  endless retry loops. Explicit Play or Refresh retries failures.
- Added original silent fixtures covering all four audio formats, native silent
  ALSA lifecycle checks, queue/repeat/autoplay coverage, and keybinding regression
  coverage. The local Linux gate passes 617 active Rust tests; formatting, strict
  Clippy, build, 704-key i18n checks, and isolated-HOME launch smoke pass.
- Linux audio adds ALSA, with `libasound2t64 | libasound2` dependency alternatives
  and complete audio license notices in Linux and macOS packages. No browser
  runtime dependency returns.
- The Spider filesystem test follow-up separates Linux raw-byte filename cases
  from portable Unix symlink coverage, correcting macOS test portability without
  changing production behavior.
- Release notes now come from this tracked changelog. Small features and fixes
  are recorded with their implementation, then checked against the full commit
  range before tagging.

**Full Changelog**: https://github.com/bencordeiro/muxal/compare/v0.5.5...v0.5.6
