# Spider map — design draft

Status: Phase 2 implemented on dev; human verification pending, 2026-10-19. The user
authorized both optional overlays and requested agent bug checks before human
review. Muxal remains implemented in Rust; the initial code-connection analysis
understands Rust, JavaScript, TypeScript (including JSX/TSX), and Python projects. No promotion or release has been approved.

## Purpose

Help the user understand a project's shape, find a part of it, and see where
recent changes are concentrated. The project root sits at the center;
directories branch outward to files. Selecting an area explains what it contains
and provides deliberate navigation to the existing editor or file browser.

The sketch in the local idea backlog separates this into three layers:

1. Project structure, size, and Git recency.
2. Optional activity and code-connection overlays (Phase 2, below).

The first layer must be useful by itself. Later layers need their own evidence
and interaction design; they are not requirements for the first release.

## Confirmed direction

- Spider is a normal, resizable pane beside agents/shells. It participates in
  the existing tab groups and layout rather than replacing the project view.
- Its active tab can expose map-specific header controls, buttons, and flags.
  Those controls must remain compact and leave normal pane operations intact.
- The visualization changer should eventually support both layout changes and
  information overlays. Introduce them gradually.
- Spider is opt-in through Settings and disabled by default.

## Proposed first-version decisions

| Decision | Proposed starting point | Reason |
| --- | --- | --- |
| Availability | Settings → Behavior → Enable Spider map, off by default | Confirmed opt-in requirement. Older settings also load with Spider off. |
| Where it lives | A regular Spider pane, using the normal tab/split/move/maximize/pop-out flows | Confirmed by the user. Compact use beside live agents is the primary experience. |
| What it maps | One selected project checkout at a time, with an explicit project-root/worktree selector | Files and changes differ between worktrees. A combined view could point at the wrong file. |
| First useful version | Structure, text-line sizes, recent Git activity, and file navigation | Delivers the original exploration goal before adding agent-specific integration. |
| Visual layout | A deterministic radial tree, with controlled expansion and folder drill-down | Preserves the spider shape while keeping large repositories navigable. |
| Visualization changer | One View menu separating layout from overlay; initially Radial plus Structure/Git activity | Layout and information are independent choices. Additional layouts can follow after compact usability is proven. |
| Refresh | Load on open; explicit Refresh; coalesced membership refresh while Recent activity is visible/enabled; retain cached data during scans | No periodic checkout polling or work without a live consumer. |

Placement and the eventual kinds of visualization controls are settled. Exact
controls, first-version modes, metrics, and visual treatment remain proposals
to validate in the prototype. The proposed first version follows the original
staged sketch. Later overlays remain optional and must preserve Muxal's light
resource footprint; provider-specific agent tracking is not a requirement.

## Opt-in setting and disabled behavior

- Add a global **Enable Spider map** toggle under Settings → Behavior, beside
  the existing optional features. The persisted `Settings.spider_enabled` boolean
  defaults to false, including when loading older settings without the field.
- While off, omit Spider creation entries from pane menus and the command
  palette. Guard action handlers too, including duplicate/split paths that would
  create another Spider instance; a stale menu or action cannot bypass the toggle.
- Turning it on makes Spider available without creating panes or scanning
  projects automatically. Scanning begins when an enabled Spider pane needs data.
- Turning it off cancels all Spider scan/Git jobs, invalidates pending results,
  and releases derived snapshots. No Spider indexing, polling, or watches run
  while it is disabled, including during workspace restoration.
- Preserve existing Spider tabs and their small configuration/view state. Show
  a lightweight disabled placeholder with **Open Settings** and normal close/tab
  operations. Disabling never closes tabs or rearranges the user's layout.
- Re-enabling restores those panes on demand with their previous scope and
  visualization choices. Any reused asynchronous result must belong to the new
  enabled generation; results from before disabling are discarded.

## Proposed first-version experience

1. Enable Spider in Settings → Behavior, then choose **Spider** through the
   existing pane-creation UI or command palette.
   Explicit New Tab/New Pane/Split choices use the normal placement behavior.
   The new pane starts on the source pane's checkout when applicable, otherwise
   the project root. Its displayed scope makes that choice visible.
2. The first view shows the root, its main areas, and a limited level of detail.
   Larger directories summarize their descendants rather than rendering every
   file immediately. A visible legend explains node size and activity color.
3. Click a node to select it. A compact detail strip shows its relative path
   and key metric. An expandable inspector shows kind, file count, recent
   commit information, and current change status when known. In a narrow pane
   it opens on demand rather than permanently consuming the canvas width.
4. Double-click/Enter/Space expands or collapses a folder within the current
   map, previewing its immediate child folders. Keep parents, siblings, and other
   open branches when readable. Only a crowded expansion narrows to that subtree;
   double-clicking the center or using Back/Backspace restores the earlier view,
   collapsed branch, and camera. **Fit** frames node and label bounds together.
5. Drag the map background to pan; use the normal pane header to move the pane.
   Zoom stays anchored to the pointer. Moving or dragging never opens a file.
   Labels appear according to available space, with selected and hovered nodes
   always identifiable. Resizing changes detail density without losing position.
6. A path filter locates files/folders and offers keyboard-selectable results;
   choosing one reveals its ancestor chain. This is navigation within the map.
7. **Open file** uses the existing editor route for the exact selected checkout:
   focus an existing editor, otherwise use a resource pane or open beside Spider.
   The map remains available. **Reveal in files** exposes the path in the
   existing file browser using an explicit checkout root.
8. Header controls follow the active tab. Start with a View menu and Fit; put
   refresh, scope selection, labels/detail options, and inspection into a compact
   overflow menu where necessary. Normal tab, split, maximize, detach, and close
   controls keep their meanings. Map keyboard shortcuts apply only while it owns
   focus; switching to an agent returns typing to that agent.

Sketch of the proposed compact placement, not a final visual design:

```text
Project sidebar | Agent pane            | [Spider] [+] [View v] [Fit] [...]
                |                       |--------------------------------
                | agent output          |                    / src
                |                       |      docs -- PROJECT
                |                       |                    \ tests
                |                       |      pan / zoom / expand
                |                       |--------------------------------
                | agent input           | root > src   metrics   [Inspect]
```

The initial prototype should compare a small project and a crowded
monorepo-shaped fixture at the existing 340-pixel minimum pane width, a typical
split width, and maximized. Check short panes too. Readability, header overflow,
expansion behavior, and navigation need user review before fixing layout rules.

### Layout versus overlay

Treat these as independent settings in one compact View menu:

- **Layout** controls geometry. Start with Radial; a branching Tree is a possible
  second layout once the shared model and interaction work. Different layouts
  use the same paths, selection, metrics, and checkout.
- **Overlay** controls what color/marks communicate. Start with a neutral
  Structure view and Git activity. Agent activity and dependency links are
  separate later choices with their own data requirements.

Changing overlays preserves camera and geometry. Changing layout preserves the
selected path and frames a useful view; there is no persistent force simulation.
Do not fill the first version with many partly functional visualization modes.

## What the visual signals mean

### Structure and node size

- Edges initially mean **contains**: directory to directory/file. A directory
  is a filesystem grouping, not a claim about a language's module boundaries.
- Use physical text-line counts for the proposed default size metric, including
  blank lines and comments. Label this **text lines**, rather than implying
  language-aware source-code analysis. Folder totals aggregate descendants once.
- Scale node area with a bounded, monotonic size function. A very large file
  must not swallow its neighbors; the inspector carries the exact known count.
  Metric changes do not reorder branches or change their angular positions.
- Binary files remain discoverable but have a small distinct representation
  and a byte count. Unreadable or skipped files have an unknown line count.
  Incomplete folder totals must be marked incomplete.
- Path identity uses native relative paths within a specific checkout. Display
  conversion must not merge distinct non-UTF-8 paths or paths that differ in case.

### Git activity

- Color means the latest observed **commit time** for a path on the selected
  checkout's current history. It does not mean complexity, importance, edit
  frequency, or that an agent is currently editing it.
- Start with a bounded recent-history window, proposed as 30 days. Show the
  window and legend. A folder's color means the latest observed commit among
  its descendants; the inspector also reports the number of recent files.
- Show uncommitted changes with a separate outline/badge, so a modification
  today is distinguishable from a recent commit. Staged, unstaged, untracked,
  deleted, renamed, and conflicted states need explicit handling.
- No Git, unborn history, insufficient shallow history, a budget limit, and a
  failed Git command are distinct states. A failed lookup must not look clean
  or old. Only claim a path is older than the window when coverage supports it.
- The first version need not trace history across renames. A rename's old/new
  paths and a deleted file can be shown as change records without attempting
  to open a nonexistent file.

## Inventory and scanning rules

- Keep every filesystem read and Git subprocess off the UI thread. Publish
  immutable snapshots to the view; render never scans or invokes Git.
- For Git checkouts, enumerate tracked files plus non-ignored untracked files.
  Tracked files remain visible even if a later ignore rule matches them. Scope
  results to the project directory when it is a subdirectory of a repository.
- For non-Git folders, use the existing `ignore` dependency for traversal and
  define the same visible dotfile policy as the file browser. Git metadata stays
  excluded. Build/vendor directories follow ignore rules; do not silently hide
  tracked project content based only on a directory name.
- Treat symlinks and nested repositories/submodules as boundaries in the first
  version. Do not follow links outside the checkout or recursively index another
  checkout. The inspector explains those boundary entries.
- Stream line counts without retaining file contents. Bound file count, bytes
  read, history records/output, and total work. Report partial results and
  exclusions rather than silently truncating the map.
- Collect recent history in batches, not one `git log` subprocess per file.
  Preserve path boundaries with NUL-delimited records and explicit error results.
- Key work by workspace, project, checkout root, and request generation. Capture
  HEAD/index context and detect an intervening checkout change before publishing
  Git results. The scan is a best-effort snapshot, not an atomic filesystem view.
- Scope changes, refreshes, and closing the final consumer cancel obsolete
  work. Also reject stale results on completion. Multiple Spider panes on the
  same checkout share an immutable scan snapshot and bounded worker; each keeps
  its own camera, mode, selection, and expansions. Repeated Refresh clicks cannot
  accumulate scanners or Git children.
- Show loading, progress/partial coverage, last successful scan, and retryable
  failure states. Preserve the last useful snapshot if refresh fails.

## Layout and rendering

- Begin with a deterministic radial hierarchy. Stable path order determines
  branch order; activity and size updates never trigger a force simulation.
- Keep a large data model separate from the much smaller visible graph. Start
  with collapsed detail, enforce a visible-node budget, and use explicit folder
  summaries when that budget is reached. Never just omit an unlabeled tail of
  files. A focused large folder also needs a usable list of its hidden entries.
- Prototype branch sectors, node spacing, and label collision behavior before
  fixing the layout algorithm. Avoid promising that every expanded tree fits
  at once. Camera state stays steady on refresh; user chooses Fit/recenter.
- Use existing pinned GPUI drawing facilities: custom element/canvas, paths,
  clipped painting, and normal controls for inspector/actions. No new graph
  engine or embedded browser is justified yet.
- Keep layout coordinates, viewport transforms, hit testing, visible-node
  selection, and label priorities pure and testable. Paint and hit testing must
  use the same transformed geometry.
- Keyboard users must be able to locate/select a path, inspect it, open it,
  return to the panes, and restore focus without precision mouse input. Status
  must remain understandable without relying on color alone.

## Fit with the current code

| Responsibility | Proposed home | Existing foundation / constraint |
| --- | --- | --- |
| Availability and enable/disable decisions | Default-false `Settings.spider_enabled` plus app action/job guards | All creation, restore, scan, and refresh paths honor the global toggle. Existing tabs remain as disabled placeholders. |
| Tree, aggregate metrics, layout, camera math, selection rules | `muxal-core::spider` | Pure functions over supplied records; no filesystem, clock, Git, GPUI, or agent provider dependency. |
| File and Git collection | App-side `spider_scan.rs`, using `integrations.rs` command conventions | `list_project_files` and `filetree.rs` establish visibility/navigation patterns, but the current list helper silently caps results and drops walk errors. Reuse suitable pieces, not that behavior. |
| Runtime view state and scan consumers | One `SpiderView` entity per pane, coordinated by `app/spider.rs` | Separate each pane's camera/selection from shared checkout snapshots and jobs. Avoid another large collection of loosely related app fields. |
| Rendering and map input | Dedicated view/element module | GPUI already has custom paint elements; no additional dependency proposed. |
| File navigation | Existing editor and file-browser routes | `open_editor_at` already reuses resource tabs. File browser currently assumes project root; worktree reveal requires an explicit root, not a path mixed into the wrong tree. |
| Pane identity and small durable configuration | `InstanceKind::Spider`, optional `SpiderConfig` on Instance, existing workspace persistence | Scope/layout/overlay belong to the pane. New fields require serde defaults. Derived tree/metrics stay out of workspace.json. |

Keep scan data in memory for the first version. If measurements later justify
a disk cache, put it under Muxal's app data and make it discardable. Spider
creates no repository files, modifies no ignore rules, and makes no Git changes.

### Ordinary pane lifecycle

Add Spider to the persisted pane-kind dispatch and live `PaneView` abstraction,
including creation, restore, content rendering, focus, duplicate, close,
detach/re-dock, and secondary windows. Audit generic/wildcard spawn paths:
historical generic spawn paths could fall through to terminal creation for
resource kinds, and some duplication/split paths assumed terminal launch. A Spider pane
owns no PTY, agent session, tmux session, or startup prompt.

Persist its checkout choice and visualization preferences. Keep camera,
selection, and expansion state per instance during the app session; prototype
feedback can decide whether any should survive restart. Duplicate creates an
independent pane with the same configuration; two maps may show different
layouts or checkouts side by side.

Represent the selected checkout separately from an agent's worktree membership,
for example `SpiderScope::ProjectRoot | Worktree(Uuid)`. Observing a worktree must
not create one, keep an unused worktree alive, or trigger disposal on map close.
If the selected worktree disappears, show it as unavailable and offer another
scope rather than silently displaying different files.

The existing 340-pixel pane minimum and pane-weight rules stay intact. Keep
secondary-window routing consistent with the existing pane tree. Keep overlay
visibility diagnostics current in `any_overlay_open`. Map inputs
that bind focus to a window need the same snapshot/rebuild discipline as editor
inputs when detached or re-docked. Closing or switching a Spider tab returns
focus to a live target. Agent-only actions/status/timers must skip it.

## Implementation checkpoints

| Checkpoint | Deliverable | Evidence before continuing |
| --- | --- | --- |
| 0 — agree on the interaction | Placement is settled; prototype a compact pane and View menu using fixed sample trees | User review beside an agent, crowded/narrow views, drill-down, selection, and file opening. This is the next proposed implementation step after planning. |
| 1 — reliable foundation | Pure tree/layout model, bounded inventory scanner, cancellation and explicit coverage states | Domain tests plus filesystem/Git fixture tests; deterministic geometry and correct checkout/path identity. |
| 2 — useful Spider in dev | Opt-in setting, real Spider pane, project/worktree scope, text sizes, Structure/Git activity views, camera, compact inspection, path navigation | Full gates; default-off and enable/disable/restore behavior works alongside normal tab/split/detach operations; user explores real projects in dev. This is the proposed first feature release boundary. |
| 3 — harden the first version | Resolve visual/performance findings, refresh/focus/window edge cases, i18n/catalogue documentation | Performance measurements and human acceptance checks below; user approves any promotion. |
| Phase 2 — recent file activity | Optional short-lived glow for observed file modifications | Measure event/checking overhead; preserve ignore/boundary rules and never imply agent attribution. |
| Later — more layouts | Add a second layout using the same data and selection model | The radial view already works well in compact panes; the alternative offers a clear practical benefit. |
| Phase 2 — code connections | Opt-in dependency layer with defined language support and edge meaning | Pick an initial language/parser and resolve imports accurately before drawing cross-links. |

These are independently reviewable development steps, not an automatic release
schedule. Use a Spider development branch and `scripts/dev.sh`; main remains
on the released version until the user approves promotion. The user chooses
versioning and publication.

## Verification and acceptance

- Verify fresh and older settings default to Spider off; creation actions cannot
  bypass it. With saved Spider tabs, disabled startup launches no scan/Git jobs.
  Disabling mid-scan cancels work and rejects late results without removing tabs;
  re-enabling restores configuration and starts only the required fresh work.
- Test real failure modes: ignored-but-tracked files, dotfiles, non-Git folders,
  empty repos, project subdirectories, non-UTF-8/spaced/newline paths, binary or
  oversized files, unreadable/disappearing files, symlink loops, submodules,
  worktree deletion, rename/delete/conflict records, shallow history, and Git
  failure. No result may escape the selected root or impersonate another path.
- Test stable ordering and aggregate counts, partial/unknown metrics, camera
  transforms, hit tests, expansion limits, refresh generation rejection, and
  cancellation. Verify switching map scope opens the correct copy of a file.
  Multiple panes share scan work without sharing camera/selection. Verify map
  close cannot trigger worktree disposal, and map restore cannot launch an agent.
- Benchmark the real muxal tree and synthetic wide/deep trees, including 10k
  and 100k paths. Initial targets: bound the visible graph to a few hundred
  nodes; aim for 60 Hz pan/zoom without scan work on the UI thread; measure first
  useful paint, indexing time, peak memory, and cancellation latency. Choose
  exact scan budgets from these measurements rather than guessing them now.
- Human review in dev: readable labels at laptop/large-monitor sizes; light/dark
  themes; trackpad/mouse/keyboard behavior; pan never opens; zoom anchor is
  stable; inspector remains usable; refresh does not unexpectedly recenter;
  the map works beside a live agent at minimum pane width and height; header
  controls overflow cleanly; layout/overlay switching preserves useful context;
  tab/split/drag/duplicate/maximize/detach/re-dock/restart all preserve the map's
  identity and scope; editor reuse keeps unsaved buffers; menus remain readable;
  focus restores in main/secondary/popped-out windows.
- Run the full AGENTS.md gate for implementation changes, isolated-HOME launch
  smoke, and sandboxed GUI testing. Update FEATURES.md when behavior actually
  exists and run extract-only i18n for added/changed UI strings.

## Phase 2 — optional overlays

The user grouped the follow-up brainstorming as **Phase 2** and authorized its
implementation on 2026-10-19. Both overlays are implemented independently and
remain disabled by default. Lightweight operation is a requirement.

- **Recent file activity:** an observed file modification starts a glow that
  fades over roughly 20–30 seconds; another edit restarts that interval. A
  visible changed file carries the glow itself; otherwise its nearest visible
  containing folder carries it. Other ancestors and the project root remain
  quiet. This means "recently changed", without naming an author/agent.
- Prefer filesystem change events and coalesced updates over repeatedly
  walking/statting the entire checkout. Modification timestamps can confirm
  changes; elapsed glow time should use a monotonic observation clock. Establish
  a baseline when enabled so opening the map does not fabricate live activity.
- Off by default; run observation only while enabled and a relevant map is
  visible. Share work per checkout, respect ignores and filesystem boundaries,
  bound event bursts, and stop timers when no visible glow remains. Handle
  atomic-save renames, deletion, new paths, and Git/build bursts. Native watches
  have resource costs too; measure large repositories before calling this cheap.
- **Code connections:** independently toggled file dependency/reference edges.
  Start with a defined language and accurate local resolution. Selection can
  show incoming/outgoing direct links, with indirect links requested separately
  to avoid overwhelming the map. Reuse existing syntax-parsing infrastructure
  where suitable; semantic references need a separate cost/accuracy decision.
- Live code connections would update after saved source changes; a recent-file
  glow needs no compiler, language server, agent hooks, or code-reference graph.

### Implemented scope and resource limits

- Recent activity uses native directory events, with a 25-second fade measured
  from observation on a monotonic clock. The activity observer reads no source contents; inventory scans still measure files.
  Atomic-save replacements count as changes to the existing file, without
  inventing a persistent temporary node or demanding a full rescan.
  Observed paths are projected onto the visible map and cached by observation,
  inventory, and visible-node identity; panning does not repeat that projection.
  Changing expansion never restarts a glow's observation clock.
- Shared observers exist only for shown, enabled consumers; tab/project/window
  visibility is reconciled after rendering. No separate animation timer is
  added: active glows use the app's existing status tick, and idle data changes
  do not repeatedly invalidate views. Disabling either optional overlay takes
  effect immediately; disabling Spider cancels all its work.
- At most four checkouts are observed, with up to 2,048 eligible directories
  each, non-recursively registered from the bounded inventory. Symlinks and
  nested checkouts remain boundaries. A reached limit, watcher failure, or lost
  event is visible and leaves manual Refresh available.
- Event batches contain at most 2,048 paths. At most 32 small batches and one
  latest graph await the UI, preserving observation times without accumulating
  older graph copies. Native events are coalesced over 100 ms; no whole-project
  walk/Git query is performed for each edit.
- Connection analysis reuses the parsers/grammars already bundled with the editor.
  The language set is capped at Rust, JavaScript, TypeScript (JSX/TSX included),
  and Python; the user requested no further languages. Rust analysis
  It handles ordinary file/inline modules, explicit file-module paths, grouped
  imports, root re-exports, and declared local Cargo dependencies/aliases.
  Unresolved relationships stay explicit; unsupported path/cfg constructs and
  imports through unknown bindings are not guessed. Analysis does not establish
  semantic symbol ownership or function-call relationships.
- Analyze at most 5,000 source/manifest files, 2 MiB per file and 32 MiB read per
  pass; initial collection has an 8-second budget. A live batch parses at most
  256 affected cached sources within a 2-second collection budget, and only
  rebuilds the graph when relevant sources change. Parsing/resolution runs on
  the observer thread, never in render or input callbacks.
- Selection lists up to 100 incoming/outgoing relations, with indirect walks
  bounded to three steps and cycle-safe traversal. Paint draws at most 48
  selected-file relations and reveals their paths within the existing visible
  node budget. Remaining hidden targets are represented by visible ancestor
  folders, with exact paths in Inspect. Dashed geometry is bounded too.
- Metrics and inventory remain snapshots. Recent activity now batches detected
  added/removed/renamed paths into an automatic inventory refresh: one second
  quiet time, at most five seconds of continuous-event deferral, and at least
  five seconds between scan starts per checkout. The existing one-second status
  tick drives scheduling; ordinary saved edits update glow/links without a scan.
  Changes arriving during a scan remain pending across observer replacement.
  Failure pauses automatic retry until manual Refresh. Hiding/disabling the
  final activity consumer cancels pending automatic work. Connections alone
  retain the manual inventory hint. Snapshot replacement preserves the camera,
  selection, expanded branches, and activity timestamps where paths still exist.
- The visible checkout label is an overview button independent of View. It
  returns to that checkout's collapsed root map; Back restores the prior view.
  View omits the redundant Checkout row when its only choice is already active;
  multiple checkouts and recovery from a missing worktree retain the selector.
- Expansion compares label crowding with the preceding overview. Unchanged
  suppression among unrelated root labels no longer forces tiny folders such
  as `.cargo` into focus. Node overlaps, clipping, and node budgets remain hard
  limits. This is evaluated for the actual pane geometry, not project size.

## Agent overlay limitation to preserve

Muxal currently knows an agent's pane, session, directory/worktree, and lifecycle
state. It does not have a general stream of per-agent file reads/writes. If two
agents share a checkout, filesystem/Git activity cannot identify which one made
a change. A working agent's presence in a worktree also does not prove it is
editing a particular file.

The user declined agent attribution on 2026-10-19; it has no value for their
workflow. Do not implement or re-propose it without a new request. File modification time can be
affected by the user, an agent, Git, generators, or a build and does not identify
the writer. Precise attribution needs provider-specific events or another
explicitly designed signal. No agent commands, handoffs, permissions, or task
dispatch should follow automatically from interacting with the map.

## Implementation review record

- Development branch: `dev/spider-map`. Settings defaults to disabled.
- Implemented: ordinary Spider panes; radial and branching-tree layouts;
  independent Structure/Git activity overlays; explicit checkout scope; bounded
  cancellable/shared scans; compact inspection and path navigation; normal pane
  operations, persistence, and disabled placeholders. The second layout reuses
  the same pure geometry and selection model.
- Pure model and real Git/filesystem fixture tests cover aggregate counts,
  native paths, camera/hit geometry, large inventories, ignore behavior,
  symlinks, binary/oversized/deleted files, nested repositories, and cancellation
  including subprocesses holding output pipes. Separate worktree snapshots keep
  the same relative filename's contents/status isolated. Non-Git folders, empty
  Git history, and Git failures have distinct coverage states.
- Phase 1 validation: `cargo fmt --all`, strict workspace Clippy, all 564 Rust tests,
  `cargo build -p muxal`, extract-only i18n and its check (622 English keys), and
  the isolated-HOME launch smoke passed. The native enabled map was inspected
  alongside a shell; a saved tab restored with an older settings file remained
  disabled and launched zero scan jobs.
- Measurements on this machine: the repository's 278-file scan took about
  0.3 seconds. The pure-model 100,000-file fixture produced 100,101 model nodes
  and 300 visible nodes in about 0.8 seconds with 53 MiB peak RSS. This measures
  model work, not interactive frame rate; pan/zoom smoothness still needs human
  review. Tree density and each branch's visible children are capped, with Find
  and paginated inspection retaining access to hidden paths.
- Initial native render inspected in a fresh sandbox with a shell beside Spider.
  The user said it looks decent for a first test. This is preliminary feedback,
  not promotion approval. Follow-up review should cover minimum pane sizes,
  controls, colors, navigation, settings disable/re-enable, and moving windows.
- Phase 2 now implements optional recent-file glows and Rust module/import
  connections. Precise agent attribution and other project-language resolvers
  remain outside this implementation. Strict workspace Clippy, 588 tests (zero
  failures; one manual benchmark excluded from the default run), the debug build,
  extract-only i18n and its check (656 keys) passed. The manual observer benchmark
  also passed: on this checkout activity-only startup was about 20 ms with 53
  watches and zero source reads; connection startup was about 281 ms with 74
  source/manifest reads, 68 Rust files, 136 links, and 6 unresolved imports. An
  earlier benchmark test process peaked at about 33 MiB RSS; this is not a measurement
  of additional GUI memory. No dependency packages/versions were added to the
  lockfile; existing parser, TOML, and native-watch dependencies are reused.
- Native fixture checks confirmed visible activity on the saved file or its
  nearest visible containing folder, a quiet root, fade/expiry, direct/indirect inspector links, and observer
  shutdown when both optional toggles are off or another pane is maximized.
  Bug checks fixed atomic-save inventory noise, stale deleted-source links,
  root re-export resolution, graph-queue growth behind a blocked UI, idle view
  invalidation, and overlapping links to collapsed folders by revealing related
  paths within the existing node budget. Phase 2 human visual review is pending.
- Expanding a directory previews its immediate child folders, so a crate can
  reveal source files rather than only a collapsed `src` bubble. Regression
  fixtures check visible source files, unchanged budgets, subtree isolation,
  and manual collapse. Refresh does not reapply expansion over a user's choices.
- Further review clarified expansion-first navigation: retain the whole map for
  small projects and narrow only if the expanded geometry is crowded. Readability
  uses at most 301 candidate nodes, the pane's node budget, padded node collisions,
  label clipping, and estimated label obstruction (over 20%, allowing one label
  in very small views). It runs on navigation, never as a polling job; resizing
  and live updates never switch scope. Return history is capped at 32 views and
  preserves camera/expansion state across detach/re-dock. These visual thresholds
  remain subject to human acceptance in small and large project fixtures.
- The isolated **Spider — navigation and glow** review workspace includes both
  a small Rust sample and Muxal's own source. Native checks confirmed two nested
  sample expansions retaining the project root, sibling branches and documents;
  double-click closes the open branch. Saving a sample file lights only its
  collapsed containing folder, then transfers to the actual file on expansion.
  Unit tests verify crowded focus and exact camera/expansion restoration. Final
  human review should assess the cutoff and navigation feel at preferred sizes.

### Inventory refresh and header follow-up

- Recent activity now triggers coalesced inventory scans for membership changes;
  ordinary edits still use the lightweight watcher. The visible checkout label
  returns to its overview with View closed. Both changes preserve navigation.
- Full gate: 593 Rust tests passed, one manual benchmark remains ignored; strict
  Clippy, debug build, formatting, extract-only i18n (659 keys), and isolated-HOME
  smoke passed. Native sandbox checks confirmed automatic additions, renames,
  removals, and no scan for an ordinary save, plus header overview navigation.
- Review build: **Spider — live refresh fixes**. Earlier running windows keep
  their older builds. Main remains untouched; promotion is not approved.

### Broad-checkout small-folder regression

- Reproduced the `.cargo` / `.github` inconsistency against Muxal: the collapsed
  overview already suppressed six labels; opening `.cargo` still suppressed six,
  but the absolute threshold incorrectly narrowed the view. Expansion now uses
  the existing suppression as its baseline while retaining hard geometry limits.
- Regression checks cover both folder opening orders at three pane sizes,
  collapse without losing other branches, and focus for genuinely overlapping
  large expansions. The complete gate passed: 595 Rust tests, strict Clippy,
  debug build, formatting, i18n (659 keys), and isolated-HOME smoke.
- The duplicate single-choice Checkout row is hidden; worktree choices and
  missing-checkout recovery remain. Native GUI checks also confirmed `.cargo` and
  `.github` expanded together with the root/siblings retained and no duplicate
  Checkout row. Review in **Spider — folder expansion fixes**.
  Main is unchanged and promotion remains unapproved.

### Basic language support completion

- Added JavaScript, TypeScript (JSX/TSX and module extension variants), and Python
  to the existing Rust resolver. No further languages are planned or requested.
- JS/TS: ordinary relative imports, re-exports, literal `import()` and CommonJS
  `require`, source extension substitution and index modules. Multiple possible
  files are unresolved; custom aliases, package exports, loaders, and computed
  runtime paths are excluded. Shadowed `require` is not treated as a module load.
- Python: `import`/`from`, aliases, relative packages, namespace children, common
  checkout/`src` roots, and package initialization/re-exports. Ambiguous search
  roots, escaping relatives, and missing local modules are unresolved. No Python
  code or environment startup is executed to discover custom search paths.
- All language facts share the existing worker, source/read limits, graph and
  navigation UI. Source saves remove obsolete edges before re-analysis. Parsing
  visits at most 200,000 syntax nodes and keeps at most 20,000 imports per source;
  JS/TS and Python each bound graph resolution to 100,000 import operations.
  The merged graph stays within 100,000 links and reports partial coverage when
  capped. Traversal and parsing add no UI-thread work or external processes.
- TypeScript resolution reference: https://www.typescriptlang.org/docs/handbook/modules/reference.html
  Python import reference: https://docs.python.org/3/reference/import.html

- Language validation: full workspace gate passed with 607 Rust tests, strict
  Clippy, build, formatting, extraction/check, and isolated-HOME smoke. Native
  mixed-language sample showed 9 parsed files and 8 links with no unresolved
  local imports; selecting App.tsx and Python main.py showed their expected
  arrows and exact paths. A native filesystem test confirmed live save/removal
  updates for TypeScript, JavaScript/JSX, and Python.

- Current repository observer benchmark after language additions: activity-only
  startup about 20 ms, 53 watches, zero source reads; connections about 281 ms,
  81 source/manifest reads, 75 source files, 145 links, 6 unresolved, no partial
  coverage flag. Native mixed-language arrows and paths were confirmed too.
  The subsequent Default Dark work passed the complete gate at 608 tests and
  does not change the Spider opt-in/default-off or observer visibility rules.
