# Changelog

User-facing changes and fixes are recorded here as they land. GitHub releases
publish the matching version section; older releases remain available on the
[releases page](https://github.com/bencordeiro/muxal/releases).

## Unreleased

## v0.5.9 — 2026-11-09

### Added

- **Live agent overview:** the dashboard is now a grid of agents and shells
  across projects, grouped by project in layout order. Tiles show the pane's
  title, status and age, worktree marker, and the bottom ten rows of its
  terminal. A summary counts working agents, requests for input, and recent
  completions; input requests outline their tile. Click a tile to open its pane.
- **Peek on hover:** rest the pointer on another agent's sidebar row for
  450 milliseconds to see its live overview tile beside the sidebar without
  switching panes. The preview stays within the window, disappears when the
  pointer leaves or the row is clicked, and takes no focus or clicks. Peeks
  belong to the main window and skip the already-focused pane and active drags.
- **Theme picker swatches:** each theme has a miniature showing its chrome,
  terminal, focused header, accent, and three status colors. Swatches appear in
  both the picker list and its closed field; the field is wider to fit them.
- **Checkout change counts:** terminal pane headers show `±N` beside the diff
  button when their worktree or project checkout has uncommitted files. Counts
  refresh about every ten seconds and disappear at zero. Panes sharing a
  checkout share its count; this is a file count, not agent attribution.
- **Flight Deck theme:** black-glass terminals in a steel-grey instrument panel,
  with magenta focus and cyan, amber, and green for working, input, and done.
  It replaces Ember Observatory; saved Ember Observatory selections migrate
  automatically. The collection remains at 24 themes.
- **Status on tabs:** agent icons use their status color, and the active tab's
  underline uses the input or done color when attention is needed. Colors adjust
  in lightness when they would otherwise blend into a themed header.

### Changed

- **Grouped top bar:** hairlines separate launch controls, saved prompts, Restart,
  pinned projects, view toggles, and app controls. Tmux and worktree options sit
  beside the agent launch button; Git diff moves right beside the dashboard.
  The duplicate top-bar Close pane button is removed; pane headers retain Close.
- **Distinct icons:** Restart, Snippets, the workspace switcher, and project
  memory now have their own glyphs, including their related menu entries.
- **Pane headers:** split controls, pane actions, maximize/pop out, and Close sit
  in spaced groups, with Close set apart. Icon buttons scale with tab-strip
  height and UI zoom; the strip itself is no taller. Tabs can be up to 280 pixels
  wide, and the diff pane's refresh control uses an icon instead of a text glyph.
- **Clearer project rows:** long branch names truncate before the project name,
  with both available in a tooltip. Memory, pin, and file-browser actions appear
  on the active or hovered project; pinned or open actions stay visible elsewhere.
- **Consistent status emphasis:** working and input stay prominent. A completed
  turn stays prominent for one day, then becomes plain muted text with a neutral
  icon; after three days its label reads `stale`. The sidebar, tabs, and overview
  use the same rule without changing the underlying agent lifecycle.
- **Working time:** status labels show elapsed time after a minute, such as
  `working · 40m`, using the current stretch of work rather than an earlier turn.
- **Shorter shell titles:** inside their project or worktree, root shells show
  their preset name, numbered when several share the root; subfolders show a
  relative path such as `./server/src`. Custom titles and paths outside the
  project remain intact. Path truncation keeps the last folder visible.
- **More useful previews:** overview and peek tiles skip divider-only lines and
  box edges, collapse repeated blank lines, and trim blank tails. Readable boxed
  content stays visible; long rows are bounded by characters rather than bytes.
- **Quieter notifications:** the sidebar Notifications section appears only
  while it contains entries.

### Fixed

- Molokai Dark's secondary labels were too dark to read, and its sidebar matched
  the background exactly. Both are corrected; the rest of its palette is retained.
- Gruvbox Dark, Gruvbox Light, and Spaceduck used the same color for working and
  input. Each now uses three distinct status colors from its own palette while
  preserving its other accents.

### Performance and development

- Overview and peek previews refresh once per second only while visible, with
  bounded rows and columns. Closed previews perform no extra terminal reads.
- Project checkout counts reuse the existing worktree refresh cycle; only the
  active project's own checkout is added, avoiding extra status scans for idle
  project checkouts. No native dependencies or external preview services are added.
- Theme status colors are optional and retain existing fallbacks. Regression
  coverage includes shell title placement, status ages and emphasis, contrast,
  authored status colors, theme swatches, and preview filtering.
- The local gate passes 654 active tests (one ignored), formatting, strict Clippy,
  build, the 705-key translation check, and isolated-HOME launch smoke.

**Full Changelog**: https://github.com/bencordeiro/muxal/compare/v0.5.8...v0.5.9

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
