# Keybindings cleanup and optional music — completed plan

Status: released and promoted as v0.5.6 after user review and authorization,
2026-10-20. The user declined searchable categories/search UI. Keybindings keep
compact friendly rows, Modified only, per-row resets, and expandable numbered
shortcuts. Autoplay is explicitly triggered by opening a project and never stops
music on focus/activity changes. Music stays fully disabled by default.
Future promotion/versioning/releases remain separate user decisions.

## Goals and confirmed direction

- Make Settings → Keybindings easier to browse without changing existing
  shortcuts, their terminal routing, or saved action identifiers.
- Add a small local music player for background listening while coding.
- Music is **fully disabled by default**, including for older configurations.
- While disabled: no music controls, directory scans, playback worker, audio
  device initialization, timers, or file observation. Turning it off stops
  playback and releases its resources; the selected folder can remain saved.
- Use the sidebar footer above **New Project** for music. Playback is app-wide,
  independent of the active project or pane. Keep the normal pane toolbar as-is.
- Keep runtime work and added dependencies small and measurable. Remaining in
  Rust fits the existing application; a second runtime is unnecessary.

## A. Keybindings cleanup

The current page creates an input for every action in one long list and displays
raw identifiers such as `FocusPrev`. It has no search or category navigation.
`Apply keybindings` currently writes even unchanged defaults, so the presence of
a stored override alone cannot identify a customized shortcut.

### Proposed page

```text
Keybindings
[ Modified only ]                  [ Apply keybindings ]
Numbered shortcuts ▸

Action                         Shortcut                   Reset
Focus previous pane            [ Ctrl+Shift+[          ]   ↺
Split right                    [ Ctrl+Shift+L          ]
…

Terminal passthrough ▸
                                      [ Apply keybindings ]
```

- A plain compact list with friendly translated names; no search or category
  filters. Numbered tab/project/agent actions remain individually editable behind
  one expandable Numbered shortcuts section. Modified only reveals customized
  numbered shortcuts even when that section is collapsed.
- One compact row per action, with consistent shortcut width and an individual
  Reset affordance only when the effective draft differs from its default.
  Modified only uses that same comparison, including unsaved edits. A reset
  follows the existing Apply flow rather than silently changing live shortcuts.
- Retain text editing for this first pass. Recording keystrokes is a separate
  interaction problem and is unnecessary for the requested layout improvement.
- Put the existing terminal-passthrough input and explanation in a labeled
  expandable subsection. Opening, editing, filtering, and closing must preserve
  input focus correctly; no hidden field can keep swallowing terminal typing.
- Preserve current blank-field/default semantics. Do not introduce an unbind
  feature, new default chords, or global duplicate-key rejection. Some actions
  intentionally share a chord in different focus contexts.

### Data and integration

- Add a small action catalogue with stable ID, translatable label,
  default shortcut, and focus context. Keep one authoritative definition of
  defaults, shared with installation and the cheat sheet rather than duplicated
  in the page. Pure catalogue/default-comparison logic can live in
  `muxal-core::keybindings`; GPUI action construction stays in the app.
- Keep draft edits keyed by action ID independently of the filtered rows.
  Filtering and scrolling never discard edits. All existing IDs and serialized
  `KeyBindingCfg` data remain compatible. Preserve unknown saved action entries
  rather than erasing them during a presentation change.
- Distinguish draft values, effective live values, and defaults. Reset removes
  the selected override when applied. Existing explicit default entries should
  display as unchanged; normalize safely without rewriting unrelated settings.
- Fix the relevant Settings rollback path: currently `cancel_settings` restores
  its snapshot and then calls `apply_keybindings`, which reads edited widgets
  back over it. Cancel must reinstall the snapshot's bindings/passthrough and
  resynchronize the widgets. This is necessary for trustworthy reset/edit UI.
- Use the existing GPUI shortcut parser/validation path where needed; preserve
  plain Ctrl+letter passthrough, scoped search, Tab handling, and editor shortcuts.

## B. Optional local music

### Placement and controls

```text
│ … project and agent list …       │
│                                 │
│  Evening rain… [⏮] [▶/Ⅱ] [⏭] [↻1] │
│  + New Project                  │
```

- A small fixed footer keeps controls reachable while the project list scrolls.
  The filename stem supplies a single truncated title with the full name on
  hover. Reuse theme text/accent colors and normal icon sizes.
- Play/Pause toggles in place; Previous and Next wrap through the folder.
  The user-requested repeat-current-track toggle is off by default and saved.
  It affects natural track endings; manual Previous/Next still change tracks.
  Changing tracks while paused keeps the player paused.
  Buttons use click handlers and keyboard focus, with translated accessible
  labels/tooltips. At the narrowest sidebar width, shorten the title first.
- With no folder, replace the title with a compact **Choose music folder**
  action. Empty, loading, and unavailable states stay understandable and disable
  unusable controls. When the sidebar is hidden, its existing toggle reveals
  the controls again; this version adds no second player in the top bar.
- Add a Music section in Settings: Enable music, folder chooser, Refresh folder,
  an output-volume slider (0–100% in 1% steps), and **Start music when opening a project** (off).
  Volume changes live, with a percentage readout, and persists on release.
  Cancel restores the opening volume, and reopening resynchronizes the handle.
  Volume stays out of the compact footer and never changes system-wide volume.

### Folder and playback policy — proposed first-version defaults

These choices make the first implementation concrete; they remain reviewable.

| Topic | Proposed behavior |
| --- | --- |
| Source | Choose one existing local folder through the native chooser. Read files in place; create no music directory, copy, project file, or generated playlist. |
| Discovery | Include supported files in nested folders, in stable relative-path order. Skip hidden children and symlinks; do not follow directory loops or crawl outside the selection. |
| Initial formats | MP3, FLAC, PCM WAV, and Ogg Vorbis, subject to the playback feasibility checkpoint. State actual supported formats in Settings. |
| Queue | Advance sequentially and repeat the folder at its end. Previous/Next wrap and keep paused playback paused. The optional repeat-current-track toggle affects natural endings only; manual navigation still changes tracks. |
| Remember | Save folder, relative last-track path, volume, repeat-current-track, and opt-ins. Resume the saved track from its beginning; ordinary launch stays paused. Do not persist a full library or audio buffers. |
| Autoplay | Only with both opt-ins enabled, on the first successful project opening of a process, after terms/workspace readiness. Enabling Music or selecting a folder does not itself start playback. |
| Respect pause | User pause wins for the rest of that process: project switches, focus changes, Settings, and later workspace entries never restart it. Turning autoplay off never starts or resumes audio. |
| Scope | One player shared by the main window, secondary project windows, and pop-outs in a process. Changing projects/workspaces does not duplicate or restart it. |
| Background | Hiding the sidebar, switching apps, or minimizing does not interrupt user-started music. Normal quit stops it; existing minimize-to-tray behavior may keep it playing. |
| Multiple processes | Independent main/dev processes remain independent; enabling music in both can produce two players. Sandbox defaults stay off. No cross-process broker is proposed. |
| Refresh | One cancellable discovery pass on enabled initialization/folder selection, then explicit Refresh. No filesystem watcher, background polling, or automatic metadata indexing. |
| Invalid track | Mark failed entries for this queue pass and advance at most once through the queue. If none play, stop with one useful error rather than retrying forever. |
| Device failure | Stop cleanly and show a concise error; retry on explicit Play. No repeated device-open loop or automatic playback after unplug/recovery. |

“Actively working” is deliberately approximated by opening a project. This does
not detect typing, agent progress, idleness, or user presence. The autoplay label
must say what actually triggers it. A literal work-activity trigger would need
another design decision rather than being inferred during implementation.

### Lightweight implementation boundary

- **Core:** `muxal-core::music` owns persisted music preferences and pure queue,
  transition, autoplay, retry, and selection decisions. Every new persisted field
  has a serde default; enabled/autoplay both default false. No I/O in core.
- **App service:** isolate discovery and audio ownership in a small `music.rs`
  module and keep UI rendering separate where helpful. Stream decoding with
  bounded buffers, not complete tracks in memory. Open only the selected track;
  do not read every audio file to obtain durations, tags, or artwork.
- Create output lazily on explicit Play or an eligible autoplay request. Before
  first playback, an enabled-but-paused player needs no audio device. A paused
  player may retain its bounded decoder/output resources for reliable resume,
  but must not run an idle UI refresh loop. Disabled retains none of them.
- Use commands/events and one owned playback service. File discovery, decoder
  setup, and shutdown must not block the GPUI thread; GPUI state changes stay on
  the UI thread. Emit track/state changes rather than repainting a progress clock.
  No seek bar or waveform is needed for this version.
- Bound discovery provisionally to 10,000 tracks, 50,000 visited entries, and
  depth 16; make cancellation effective during traversal. Report partial results
  explicitly if a limit is reached. Benchmark before treating these as final.
- Folder changes/disable/quit invalidate pending discovery and decoder results
  with a generation token. Late completion can never restart disabled music.
  Disabling is immediate from the user's perspective and clears the active queue.
- Settings Save/Cancel must reconcile preferences with the service. Cancel can
  restore a previous folder/configuration but never resurrect playback that the
  user explicitly paused or stopped by disabling. Turning Music off retains its
  remembered folder while releasing active resources.

### Playback dependency feasibility checkpoint

There is no existing music playback stack in the current app dependencies.
**Rodio 0.22.2 was selected with explicit playback/MP3/FLAC/Vorbis/WAV features.** Its documented
playback controls cover pause/resume/skip; its release features allow selecting
playback and specific decoders while disabling defaults, which otherwise include
recording and more codecs. Inspect the exact released Cargo feature graph before
adoption rather than relying on an evolving upstream master API.
See [Rodio controls](https://docs.rs/rodio/0.22.2/rodio/struct.Player.html)
and [release feature flags](https://docs.rs/crate/rodio/0.22.2/features).

1. Record baseline release binary/package size, cold-start behavior, idle CPU/RSS,
   and native dependency closure. Prototype minimal playback off the UI thread
   in the dev branch using explicit playback/decoder features; no recorder.
2. Compare disabled, enabled/paused, playing, paused-after-play, and disabled-again
   against baseline. Disabled should add no running audio service, open device,
   recurring music tasks, or measurable continuing CPU work. Report the binary
   and package growth separately: runtime opt-out does not remove linked code.
3. Validate Linux and macOS output/device-failure behavior. CPAL documents ALSA
   as its default Linux backend and CoreAudio on macOS; check the exact pinned
   dependency's requirements and native linking before choosing a backend.
   See [CPAL platform/build documentation](https://github.com/RustAudio/cpal#linux-build-dependencies).
4. Audit licenses/provenance, CI system packages, .rpm/AppImage behavior, and the
   hand-maintained .deb dependencies using `dpkg-shlibdeps`. Preserve current
   Ubuntu/Debian compatibility, with t64 alternatives wherever required. Do not
   assume an app setting removes an unconditional native library requirement.
5. Present measured dependency/package cost at this checkpoint. If it is
   disproportionate or complicates supported installs, stop and evaluate a
   smaller approach with the user before expanding scope. Do not silently fall
   back to shelling out to an installed player or adding a browser/media runtime.

No streaming accounts, downloads, bundled music, cover-art library, microphone
capture, project-specific players, or agent/typing monitoring are in this draft.

## Implementation order and acceptance

1. **Keybindings checkpoint:** defaults/reset logic, compact page,
   reset and rollback correctness. Update FEATURES and extract-only catalogs in
   the implementation change. Run the full gate, then user review in dev.
2. **Playback feasibility checkpoint:** measure a minimal backend and packaging
   changes before investing in the full music UI. Keep it on dev, opt-in, with
   no unsolicited audible demonstration.
3. **Music checkpoint:** settings/persistence, bounded folder discovery, queue
   decisions, lazy playback, footer controls, shutdown and failure handling.
   Update FEATURES, any affected install/license docs, and extract-only catalogs.
4. **Human verification:** the user judges layout, chooses their own music, and
   checks audio. No desktop automation or screenshots for basic visual review.
5. **Finish:** address review findings, repeat the affected gates, then wait for
   explicit promotion approval. Versioning and a new release are separate user
   decisions; never restart or kill their running main.

### Meaningful automated coverage

- All existing action IDs/defaults/contexts remain represented; friendly labels,
  normalization, and modified/reset behavior agree.
- Hidden rows retain draft edits; Apply updates intended bindings; Cancel restores
  the exact snapshot, including terminal passthrough and unrelated custom data.
- Old settings load with music and autoplay off. Save/load round-trips preferences
  without serializing queue/runtime state. Disabled restoration creates no service.
- Pure queue tests cover pause/skip, end-of-folder repeat, missing saved tracks,
  exhaustion of invalid entries, autoplay once, and explicit pause dominance.
- Service tests use an injected backend/reader to verify lazy device creation,
  bounded/cancellable discovery, stale result rejection, shared ownership, disable,
  folder change, Settings rollback, and quit without requiring speakers in CI.
- Deterministic generated audio fixtures verify actual decoding for every promised
  format without downloaded copyrighted music. Keep real audible playback for the
  user's dev review; test device errors and stop/release paths too.

After implementation changes, run `cargo fmt --all`, strict workspace Clippy,
`cargo test --workspace`, `cargo build -p muxal`, i18n extraction/check, and the
isolated-HOME launch smoke. Only `scripts/dev.sh` launches interactive review.

### Compact human checklist

- Browse friendly shortcut rows, expand numbered shortcuts, and try Modified only.
- Edit/reset a shortcut, change filters, Apply; repeat with Cancel and confirm the
  original shortcut and agent Ctrl-key passthrough still work.
- Fresh/disabled Music adds no sidebar row and starts no playback or scanning.
- Choose music, play/pause, Previous/Next, repeat-track, and verify the footer at narrow sidebar widths,
  larger UI scales, and in both light/dark themes.
- Switch panes/projects/windows and hide the sidebar: one continuous player.
- Restart with autoplay off, then test its explicit opt-in; a manual pause wins.
- Try an empty/missing folder, an invalid file, and unavailable output; recover
  through an explicit action without a repeating error or busy loop.
- Disable during discovery/playback and quit normally: work/audio stop cleanly.

## Draft decisions to revisit during review

The placement and disabled-by-default requirement are settled. Folder repeat,
the initial format list, recursive discovery limits, autoplay-on-project-open,
and the exact Settings layout are proposed defaults. No remaining choice blocks
reviewing this plan; none should be mistaken for a shipped feature.


## Implementation measurements and follow-up

- Final Linux release comparison, stripping all symbols equally:
  88,905,216 → 90,541,568 bytes (+1,636,352 bytes / 1.84%).
  Gzip-compressed binaries at level 6: 22,554,915 → 22,990,231 bytes
  (+435,316 bytes, about 425 KiB). This measures binaries, not a whole install.
- `ldd` adds `libasound.so.2`; `dpkg-shlibdeps` confirms its minimum 1.0.29.
  The hand-maintained .deb list uses `libasound2t64 | libasound2` alternatives.
  Existing CI already installs ALSA build/runtime packages. No retired browser
  stack returned. Measurements are local Linux results, not macOS verification.
- Four tiny original silence fixtures test actual decoding without speakers or
  downloaded music. `scripts/generate-music-fixtures.py` regenerates them using
  local libsndfile; that tool is not an app/test runtime dependency.
- Navigation/workflow sound effects are the user's next idea: soft, subtle system
  sounds. They are deferred from this change; design their opt-in behavior and
  event choices separately after music/keybinding human review.

- Linux verification: 615 active Rust tests pass (one manual benchmark ignored),
  strict Clippy, debug/release builds, current extract-only catalogs, and private
  HOME launch smoke. Disabled smoke has no music worker or sound-device handles.
  A subprocess with an isolated ALSA null-device config tests native open,
  pause/resume, volume, and release without physical speakers. No audible music
  or desktop automation was used. Native macOS listening remains to be checked.

- The initial local .deb built with all five audio notice files, reported the
  unchanged 0.5.5 development version, and resolved all linked libraries. The
  review sandbox is
  `.muxal-dev/music-review`, launched using `scripts/dev.sh`, with a dedicated
  Music & keybindings review workspace and no preconfigured agents.

- Requested follow-up: Previous and repeat-current-track now use the same small
  footer. Repeat defaults off, persists, and applies immediately without queueing
  a mode command; Settings Cancel restores it with the other music preferences.
  Previous wraps and respects pause. Added pure queue/persistence and worker
  tests for natural-end repeat, turning repeat off, and manual navigation while
  repeat is on. The follow-up gate passes 617 active tests, strict Clippy, the
  debug build, current 704-key extraction/check, and isolated-HOME launch smoke.

- Release verification: v0.5.6 is published; all ten package checksums pass.
  Native release builds and CI pass on Linux x86/ARM and universal macOS. CI runs
  617 active tests on each Linux architecture and 613 on macOS. The published
  Linux binary reports 0.5.6 and retains the glibc 2.35 baseline; Mac bundle
  versions/architectures and packaged audio notices are verified. The user's
  final volume request uses a 0–100% slider with one-percent steps, live readout,
  live playback changes, and save on release. The release notes come directly
  from the tracked changelog. Atomic promotion is complete; the installed main
  reports 0.5.6, and the user restarts it themselves.
