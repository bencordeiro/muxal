# Changelog

User-facing changes and fixes are recorded here as they land. GitHub releases
publish the matching version section; older releases remain available on the
[releases page](https://github.com/bencordeiro/muxal/releases).

## Unreleased

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
