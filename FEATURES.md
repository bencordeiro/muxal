# muxal — Features

muxal is a GPUI-based, multi-agent terminal multiplexer: run several coding agents
(Claude, opencode, Amp, …) and shells side by side in a tiled, tabbed workspace
with first-class git worktrees, agent status tracking, and notifications.

This file is the canonical catalogue of what muxal can do. **When a user-facing
feature is added or changed, update the matching entry here in the same change**
(see `AGENTS.md`).

## Panes & layout

- **Recursive split layout** — panes form a horizontal/vertical split tree; any
  pane can be split again, nesting freely.
- **Resizable splits** — drag the divider between panes; sizes persist per project.
  Cached terminals receive the final size after the drag, so responsive TUIs redraw
  without requiring a keypress or scroll.
- **Minimum pane width** — panes can't shrink so narrow that an agent TUI becomes
  unusable (keeps a sane terminal width).
- **Scrollable pane area** — because panes have that minimum width, a layout with
  enough panes side by side needs more width than a small display has — most often
  a layout built on a large monitor and then reopened on a laptop. The pane area
  scrolls horizontally in that case, so the
  panes that don't fit can still be reached instead of falling off the right edge.
  Panes still shrink to fill a window they do fit in; scrolling only begins once
  they can shrink no further. Drag its scrollbar, swipe a trackpad sideways, or
  hold **Shift** and use the wheel — a plain wheel is left to the pane under the
  pointer, so scrolling an agent's scrollback never drags the strip sideways.
- **Drag-to-dock split (Zed-style)** — drag a tab onto a pane edge to pull it out
  into a new split, or onto the center to add it as a tab; drag a pane by its
  title bar to relocate the whole pane, with a highlighted drop zone.
  Rearranging never resizes the panes you didn't touch: dividers stay anchored,
  the dropped pane shares the drop target's region, and the span it vacates is
  absorbed where it left — so undoing a drag lands back on the exact previous
  split without nudging anything.
- **Swap panes** — drop a dragged pane on another pane's center to swap their
  positions.
- **Per-project focus** — switching away from a project and back restores the
  pane that was last focused there, falling back safely if that pane was removed.
- **Maximize** — temporarily expand one pane to fill the work area. The whole tab
  group comes along: every tab stays in the tab bar and switching tabs keeps the
  pane maximized, as does closing or popping out the tab being shown while others
  remain. While a pane is maximized, selecting another agent (sidebar,
  notification, tray) moves the maximize to it, so that agent comes up full-size
  instead of hiding behind.
- **Pane cards** — rounded "card" panes with an accent ring + glow on the active
  pane, hover highlight, and a configurable border style.

## Tabs

- **Tabs per pane** — each pane is a tab group; `Ctrl+T` opens a new tab in the
  active pane.
- **Non-blocking agent creation** — a new agent tab paints and focuses first; PTY
  creation and child startup run on a worker, with stale launches discarded if the
  tab closes or is replaced before startup completes.
- **Direct pane creation controls** — the pane-local tab button opens the agent
  picker and creates the chosen agent in that tab group. Split buttons clone the
  shown pane's agent in one click; right-click opens the alternate-agent picker.
- **Drag & reorder** — drag tab pills to reorder within a pane, move them to other
  panes, or drop at a precise insertion point.
- **Pinned tabs** — pin a tab to the leftmost block; pins behave fluidly when
  dragged past unpinned tabs.
- **Tab context menu** — right-click a tab to Rename, Duplicate, Pin/Unpin, Restart
  agent, Close tabs to the left / right, Close others, or Close. Duplicate clears
  copied conversation state so a harness can mint a new session instead of
  deliberately resuming the source. **Restart agent** (resume-capable agents only)
  does what the toolbar Restart does for that tab.
- **Restart** — the toolbar Restart (also the tab menu's *Restart agent* and the
  palette's *Restart agent*) stops the pane's process *and* its tmux session, then
  relaunches in place in a fresh session. A resume-capable agent comes back on its
  saved conversation, so a newer harness binary takes effect without losing it; a
  shell or other program starts over.
- **Tab cycling** — keyboard shortcuts cycle to the next/previous tab.

## Pop-out windows

- **Detach a pane** — pop a pane out into its own OS window without terminating it.
  Terminals and editors move across; a browser pane is re-created in the new window
  at the same URL (a native webview belongs to the window that built it).
- **Re-dock in place** — a popped-out pane remembers where it came from; the Dock
  button returns it to its original location.
- **Close terminates** — closing a pop-out window kills its terminal (with a
  confirmation). Editors and browser panes just close.

## Agents

- **Built-in agent presets** — Shell, Claude, opencode, Amp (ampcode), Grok
  (x.ai), Hermes, Ollama, **Ollama Code**, Pi, and a **Browser** preset, each with
  its own icon. A preset is either a terminal agent or a **Browser** (opens a
  browser pane at its homepage) — switch the type and edit the fields in
  Settings → Agents.
- **Ollama Code** — runs a coding agent backed by an Ollama model via
  `ollama launch <agent> --model <model>` (seeded as `ollama launch opencode
  --model glm-5.2:cloud`); change the agent or model in the preset's args.
- **Configurable launch** — per agent: program, model + model flag, effort +
  effort flag, extra args (shell-style quoting: `--append-system-prompt "be terse"`
  stays one argument, matching the iOS companion's parser; an unbalanced quote
  falls back to space-splitting with a feed warning), environment variables,
  system-prompt injection
  (via a CLI flag or by typing it in at startup), and a runner startup delay
  (ms to wait after the agent's first output before a runner types — for slow
  starters like opencode; 0 = auto-wait until output goes quiet).
- **Installed-binary autodetect** — agents whose binary isn't on `PATH` are hidden
  from the new-agent menus and marked "not installed" in settings; they reappear
  automatically once installed. A GUI launch reconstructs the bin dirs the desktop
  environment otherwise omits — on macOS a Dock/Finder launch restores Homebrew and
  `~/.local/bin`; on Linux a desktop-entry / AppImage launch restores `~/.local/bin`,
  `~/.opencode/bin` (opencode's installer default), Linuxbrew, and friends — so
  agents are detected and spawnable the same as from a terminal. Version managers
  are covered too: nvm's versioned node bin dir (`~/.nvm/versions/node/vX.Y.Z/bin`,
  newest installed version wins) and mise's shim dir (`~/.local/share/mise/shims`)
  are discovered at startup, so npm-global and mise-managed agents (`pi`,
  `claude`, `opencode`, …) resolve under a GUI launch as well.
- **Terminal font auto-resolution** — the terminal never renders with a
  silently-substituted proportional face: muxal resolves a family that is
  installed and verifiably fixed-pitch (advance probe), warns when the
  configured family had to be substituted, and falls back per-glyph to
  installed mono faces only. Fixes the spaced-out glyph rendering on distros
  without DejaVu Sans Mono (Arch/Omarchy).
- **Graceful launch failure** — if an agent can't be spawned, the pane falls back
  to a shell showing the underlying error instead of crashing. If even the
  fallback shell can't start, the pane shows the failure in place (the toolbar
  Restart retries) and the error lands in the NOTIFICATIONS feed.
- **Session resume** — resume-capable agents reopen their prior conversation after
  a muxal restart. Two shapes, both configurable per preset:
  - **Host-minted** (Claude, Grok): `session_id_flag` + `resume_flag` — muxal
    launches with `--session-id` the first time and `--resume` on restart.
  - **Agent-minted** (Codex): only `resume_flag` (`resume`) — first launch is bare;
    muxal captures the UUID Codex publishes for that pane, validates it against
    `~/.codex/sessions` before restart, and relaunches as `codex resume <id>`. Multiple Codex
    panes can share a project without resuming one another's conversations.
  **Conversation switches stick** — an agent can swap its conversation inside a live
  pane (`/resume`, `/clear`, a fork) without restarting the PTY, and muxal rebinds the
  pane to what's actually on screen instead of the conversation it launched.
  Claude panes learn the switch from a process-local `SessionStart` hook keyed to the
  pane; Codex panes learn it from a later OSC title. Both are accepted only after the
  new UUID's own on-disk session proves it belongs to this pane's directory and that no
  sibling pane already owns it — terminal titles carry no sender identity, so an
  unverified one cannot rebind a pane. If the saved session is gone, the pane
  quietly starts fresh.
- **Broadcast** — `Ctrl+Shift+I` opens a broadcast bar; type a line and Enter (or
  Send) writes it + a newline to every agent pane in the active project at once.
- **Shared project memory** — opt-in per project: agents are told (via their system
  prompt) to `grep` and add durable lessons to a `.muxal/MEMORY.md` file shared
  across every agent and run in that project. muxal creates the file, git-ignores
  `.muxal/`, and agents get its path in a `MUXAL_MEMORY_FILE` env var. Enable it
  on a project (sidebar right-click or
  Settings → Projects); a memory button on the project row opens the manager. Plain
  shells are skipped.
- **Self-maintaining memory** — each fact is one `##` section carrying a machine
  meta line (id, dates, tags). muxal keeps the file **most-relevant-first** (recently
  used entries and 📌 pinned ones rise to the top), **timestamps** every entry,
  **auto-purges** un-pinned entries unused for 30 days, and **caps** it at 40
  un-pinned entries (evicting the least-recently-used) — so it stays small and
  greppable and never needs hand-pruning. A legacy flat `MEMORY.md` is imported, not
  lost, on first open.
- **Memory panel** — the project row's memory button toggles a docked, resizable
  second-sidebar panel (like the file browser, sharing its slot; width persists per
  workspace): search/grep the entries, add one (title + note + tags), pin/unpin
  (pinned entries are exempt from purge and cap), delete (with confirm), or open the
  raw `MEMORY.md` in the editor.

## Agent status

- **Real lifecycle badges** — each pane shows **working**, **idle**, **blocked**,
  or **done**, color-coded (blue / gray / amber / green) on the tab pill, sidebar
  icon, dashboard, and notification dots. A marker-based agent whose turn finishes
  is held at **done** until the agent works again — even if it never rang the bell —
  and that completion survives a muxal restart. Focusing the pane marks its
  notification read without rewriting lifecycle state or age. Sidebar badges include
  coarse age while work is blocked or done, show recent idle activity briefly
  (`idle · 12m`), omit ordinary middle age, and call out panes idle for three days
  or more (`stale · 4d`). Long pane titles ellipsize before the badge instead of
  pushing status out of the sidebar. Title-derived lifecycle and automatic-name
  updates on known agent panes are accepted only from that provider's semantic
  title shape; existing marker, bell, and process-exit signals still apply.
  Provider-owned screen rows can add precise state that titles omit: Claude
  permission forms report **blocked**, while positive background counts displayed
  by Claude and Grok remain **working** even when their titles look idle. Claude's
  visible foreground progress row also preserves **working** when a narrow tmux
  pane truncates the usual interrupt marker. Brief
  Grok redraw gaps do not forge completion. Ambiguous waiting rows do not claim
  active work. Codex `/rename` values are read
  from its session index by the pane's captured session UUID, so child commands
  such as `npm` cannot replace the session name or forge a title-derived state
  transition.
- **Per-agent detection markers** — status is inferred from on-screen TUI markers
  (e.g. Claude's "esc to interrupt" spinner, a permission prompt), with built-in
  defaults per agent and **editable working/blocked markers per preset**.
- **Heuristic fallback** — agents without markers fall back to bell + output
  activity. They reach **done** only from the bell or process exit, never from a
  quiet spell — so an incidental redraw (e.g. a focus repaint when you click the
  pane) can't fake a finished turn.
- **Auto-continue** — an agent that lays out a multi-phase plan sometimes finishes
  the first phase and just stops, waiting, with the todo list still half-unchecked.
  Each agent pane has an **Auto** toggle in its header: while it's on, muxal watches
  the pane and, whenever the agent goes idle with work still to do, types `continue`
  and presses Enter for you. It fires when it can see pending work — Claude's `☐`
  checkboxes or an "N pending" count — or when the agent voluntarily stops to check
  in ("My recommendation is to pause here.", "Shall I continue?", "I'd hold here
  unless you want that scaled run."), so a plan keeps moving without you babysitting
  it. "Idle" is judged by the screen going still
  rather than by a status guess, so it won't fire over an agent that's plainly
  mid-work (spinner still turning) even if its "working" marker isn't recognized;
  and the *next* nudge keys off the todo list actually moving, so it follows a
  multi-phase plan even when a phase finishes and the agent re-pauses in a blink. It
  never nudges a genuinely-finished agent, and never answers a permission prompt
  (that needs your real yes/no). If `continue` fires a few times and the screen
  never changes at all — a dead loop, like an agent erroring out the instant it
  resumes — it stands down on its own and posts a notification rather than
  hammering forever; but an agent that keeps answering with fresh work (even when
  its remaining tasks are blocked on you and no checkbox moves) is left running.
  When the agent says it's out of work it can do ("no responsible work left",
  "nothing further I can do"), auto-continue stops and notifies you rather than
  nudging a finished agent in circles. Runtime-only: it's off again after a restart.

## Git worktrees

- **First-class worktrees** — named, color-coded git worktrees shared by one or
  more panes; toggle "create a git worktree" when spawning.
- **Inheritance** — a new tab or split joins its pane's worktree; a duplicate
  inherits the source's; otherwise a fresh worktree is created (toggle on).
- **Visual coding** — the pane outline + glow tint to the worktree color, a name
  badge on a uniform pane, a per-tab color dot, and a matching dot in the sidebar.
- **Sidebar grouping** — panes are grouped under colored worktree subheaders;
  rename a worktree inline or from the context menu.
- **Dispose flow** — when a worktree's last pane closes (or its agent exits), a
  clean worktree is removed silently; otherwise a modal offers **Commit & close**,
  **Merge & close**, **Discard**, or **Keep**.
- **Unmerged-commit detection** — the dispose flow also catches commits not yet in
  the base branch (not just uncommitted changes), so committed work isn't silently
  orphaned; Merge lands them on the base.
- **Kept worktrees** — "kept" (detached) worktrees stay in the sidebar and can be
  resumed (spawn a new agent into them) or resolved later.
- **Review workflow** — each worktree shows its uncommitted-change count;
  right-click for **View changes** (opens the git-diff pane), **Review** /
  **Security Review** (spawns that runner *inside* the worktree to review its
  diff), **Discard changes** (reset the worktree to its base, keeping it), or
  **Discard worktree** (close its panes + delete the worktree and branch).
- **GitHub PRs** — when the `gh` CLI is installed, the worktree menu also offers
  **Push branch**, **Create PR…** (push + open the PR-create page), and **Open PR**
  (open the branch's PR in a browser); these run off the main thread and toast the
  result.

## Runners

- **One-click task launchers** — predefined runners (e.g. Review, Security Review)
  spawn an agent that auto-types a task prompt. The toolbar "Run task" dropdown
  lists them; click to run, or the pencil to edit one in Settings → Runners.
- **Templated prompts** — `{{input}}` is substituted with run-time details.
- **Auto mode** — send a configurable number of Shift+Tab presses (then Enter) at
  startup to reach auto-accept mode.
- **Ephemeral + restore-safe** — on app restore a runner re-types its prompt but
  does not auto-submit.

## Snippets

- **Type saved text into an existing pane** — reusable snippets (a managed list in
  Settings → Snippets) are typed straight into an already-running pane, unlike
  runners (which spawn a new agent). Each snippet records whether it **auto-submits**
  (presses Enter) or just drops the text into the input for you to review.
- **Three ways to send** — the toolbar **Snippets** dropdown (sends to the active
  pane), the command palette (*Send snippet: …*), or right-click a terminal tab →
  **Send snippet** (sends to that specific pane). Multi-line text goes in via a
  bracketed-paste-aware insert so it won't submit on its own newlines.

## Loops

- **Scheduled task launchers** — run a saved prompt on a chosen agent in a chosen
  project on a timer: every N minutes, every N hours, or daily at a local time.
- **Unattended firing** — when due, a loop spawns a fresh agent as its own new
  pane appended at the end of its project's layout, types the prompt, optionally
  sends auto-mode Shift+Tab presses, and respects the agent's startup delay (so
  opencode works). The pane is **visible but not focused** and never switches your
  active project — a loop firing on a timer can't interrupt what you're typing.
- **Post-run policy** — leave the agent running, or exit it once it finishes its
  turn (with a max-runtime safety cap). A still-running loop won't stack a second
  copy.
- **Managed from the main window** — a toolbar "Loops" dropdown lists your loops:
  click one to run it now, the pencil to edit it in Settings → Loops, or "New
  loop…" to create one. Schedules survive restarts (a daily-at whose time passed
  while closed fires once on next launch). Loops fire only while muxal is running.

## Built-in browser

- **System webview, not bundled Chromium** — preview links agents print (or a
  locally hosted dev site) without leaving muxal. Uses the OS engine (WKWebView on
  macOS, WebKitGTK on Linux), so it's light on disk and memory.
- **macOS: an embedded pane** — ctrl+click a URL and it opens as a browser pane
  beside the terminal, with an address bar and Back / Forward / Reload buttons;
  Reload refreshes the page you are actually on (several links deep, if that's where
  you are), not the pane's original URL. The URL persists and restores with the
  workspace. Clicking into the page makes it the active pane — so paste and the
  toolbar act on the browser, not on whichever pane you were in before — and hands
  it the keyboard; muxal's own shortcuts keep working until you click into a page.
  The toolbar can open the current page in the system browser. Native page context menus remain usable.
- **Linux: a separate browser window** — gpui can't embed WebKitGTK, so links open
  in a muxal-managed browser window (a crash-isolated `muxal --browser` process);
  if WebKit isn't installed it falls back to the system browser with a note.
- **Browser as a preset** — the built-in **Browser** preset opens a web-browser
  pane; pick it anywhere you pick an agent (the toolbar's new-pane dropdown, or
  hold a pane's split / `+` button and choose it). Configure its homepage — and
  add more browser presets with their own homepages — in **Settings → Agents**
  (default `duckduckgo.com`; a bare domain gets `https://`). On macOS it's an
  embedded pane in the layout; on Linux it opens in a separate browser window.
- **Optional** — Settings → Behavior → "Open ctrl+clicked links in the built-in
  browser" (default on); off routes every link to the system browser.

## Notifications

- **Desktop notifications** — fired when an agent finishes a turn or needs attention
  (a blocked prompt / the terminal bell), but only while muxal's window isn't
  focused — no toast pops over the app you're already looking at (the in-app feed
  still records it). Clicking the notification raises muxal and jumps to the pane
  that fired it.
- **In-app NOTIFICATIONS sidebar** — a category above PROJECTS collecting agent
  events **and** all app messages (git results and save errors —
  workspace, settings, workspace list, project memory, and layout backups —
  everything that used to be a pop-up toast goes here instead; persistent save
  failures report once per cause, not on every autosave). Agent rows are
  click-to-navigate (jump to the pane + dismiss); all rows are individually
  dismissable, with a clear-all. Collected even when desktop notifications are off.
- **Controls** — an enable/disable toggle and a "send test notification" button.
- **System tray** (Settings → Behavior → "Minimize to the system tray on close") —
  closing the window iconifies muxal to a tray icon instead of quitting. The tray
  menu lists every agent with its live status and the most recent notifications;
  clicking one restores muxal and focuses that project + pane, and "Quit" exits for
  real. Linux uses StatusNotifierItem (needs an AppIndicator/SNI host — standard on
  KDE, the AppIndicator extension on GNOME); macOS uses the status-bar item. (Stock
  GPUI can only iconify, so the window still appears in the
  taskbar; restoring from the tray is best-effort on Wayland — the dash always works.)
- **Developer console** (Settings → Behavior → "Developer console", toggled with F12) —
  an opt-in popped-out window logging errors as they happen. A failed agent launch
  shows the program it tried, the working directory, and the OS error/code; git and
  save errors land here too. Timestamped, newest first, selectable/copyable, with a
  Clear button. F12 is a no-op until the setting is enabled.

## Terminal

- **alacritty-based emulator** — full VTE terminal with truecolor support.
- **Selection & clipboard** — mouse text selection and copy/paste (`⌘C`/`⌘V` on
  macOS, `Ctrl+Shift+C`/`Ctrl+Shift+V` elsewhere). A global Settings → Behavior
  choice picks the mouse copy/paste style: **right-click copy/paste** (default —
  right-click copies the selection, or pastes when nothing is selected), a
  **right-click Copy/Paste menu**, or **copy on select** (selecting copies
  immediately; right-click pastes). **Paste**: plain `Ctrl+V` is host-side smart
  paste — text and file paths go into the PTY; an image is forwarded as raw
  Ctrl+V (`0x16`) so agents that read the OS clipboard (Grok) can attach it.
  `Shift+Insert` pastes and `Ctrl+Insert` copies. File **drag-and-drop** pastes
  shell-quoted paths into the focused terminal.
- **Mouse reporting** — when an app enables mouse mode (Grok, Claude, vim, …),
  clicks, drags, and motion are forwarded as SGR/X10 mouse events so the app can
  focus its prompt, scroll its own pane, set the cursor, etc. Hold **Shift** to
  force local text selection instead. The wheel already forwarded to mouse-aware
  apps; button reports complete that path.
- **Scrollback** — history with a draggable overlay scrollbar; clear it via
  `Ctrl+Shift+K` or the tab's "Clear scrollback" menu item. The mouse wheel
  scrolls history, or — for full-screen apps that enable mouse reporting
  (opencode, grok, vim, tmux) — is forwarded to the app so it scrolls its own
  content (tmux mouse mode is turned on automatically for tmux-backed panes).
- **Scrollback search** — `Ctrl+Shift+F` (while a terminal is focused) opens a
  search bar that highlights matches and jumps through them (Enter / ↑ / ↓),
  scanning the full history.
- **Clickable links** — `Ctrl`/`Cmd`+click opens what's under the cursor: an
  `http(s)` URL, an OSC 8 hyperlink (e.g. `ls --hyperlink` or agent markdown
  links), a literal Markdown inline link, a `file://` URI, or a **file path**
  (absolute, `~/`, or relative to the pane's working directory).
  Local files open in a muxal editor pane; only paths that exist are
  clickable, and a trailing `:line:col` is understood. `Ctrl`/`Cmd`+hover
  underlines the link and shows a pointing-hand cursor (Ctrl/Cmd down re-hit-tests
  without requiring a mouse move).
- **Links in new tabs** — middle-clicking a terminal link opens it without stealing
  terminal focus; browser target=_blank, window.open, Ctrl+click, and middle-click
  requests stay in the source project and open as Muxal tabs. Right-clicking a
  terminal link opens link actions instead of copying or pasting through the PTY.
- **Focus reporting** — forwards focus in/out to the PTY (DECSET 1004) so agents
  know when their pane is active.
- **OSC-52 clipboard** — programs in the terminal (tmux-backed panes included) can
  copy to the system clipboard via `OSC 52`; clipboard *reads* are answered with
  an empty reply, so a program can probe for support but never see your clipboard.
- **Color queries** — answers `OSC 10/11/12` and `OSC 4` color queries from the
  active theme's terminal palette, so TUIs detect dark/light mode correctly (and
  the answer always matches what's painted). Replies are generated directly on
  the PTY reader thread, while the requesting TUI is still waiting for them.
- **Exit codes** — a pane's child exit status is captured, so close-on-exit and
  session-resume recovery can tell a clean `exit` from a crash (a deliberate quit
  no longer triggers resume recovery).
- **Crash tombstones** — a pane whose process dies abnormally (non-zero exit, or
  the PTY failing outright) is never auto-closed: it keeps its final screen under
  a "process exited — code N" banner, fires an error in the NOTIFICATIONS feed
  (plus a desktop notification when unattended), and Restart relaunches in place
  (resuming a resume-capable agent's conversation).
  Only a clean exit (code 0) qualifies for auto-close. A process that was *killed*
  is named as such — "process killed — signal Hangup/Killed/Terminated" — instead
  of being reported as a crash, since the OS gives a signalled child no exit code
  of its own and it would otherwise be indistinguishable from `exit(1)`.
- **Event log** — pane lifecycle events (every exit with its code and signal, every
  close, auto-closes, PTY read errors) are appended to `muxal.log` in the data dir
  (rotated at 1 MB), so "why did this pane disappear?" is answerable even when
  the app runs with stderr discarded.
- **Content inset** — a small margin around the grid so a too-wide TUI truncates
  inside the pane rather than against the border.
- **Key routing** — `Tab` / `Shift+Tab` go to the focused terminal rather than
  moving UI focus. `Shift+Enter` / `Alt+Enter` send `ESC CR` so agent TUIs
  (Grok, etc.) can insert a soft newline; plain `Enter` stays CR (submit).
- **Agent-first plain Ctrl+letter** — muxal app shortcuts that are plain
  `Ctrl+A`…`Ctrl+Z` (no Shift) do **not** fire while a terminal is focused, so
  agents receive them as normal C0 chords (Claude `Ctrl+S` stash, shell
  `Ctrl+R`, …). Muxal chrome prefers `Ctrl+Shift+*`. Exceptions that stay global
  in a terminal: `Ctrl+T` (new tab). `Ctrl+P` is special-cased (palette only when
  no terminal is focused). Extra chords can still be listed under Settings →
  Keybindings → terminal passthrough.
- **Ctrl+P shared with the agent** — the command palette is on `Ctrl+Shift+P`
  (always), while `Ctrl+P` opens it only when no terminal is focused — so a focused
  agent (e.g. opencode) receives it. Deselect the pane (click the toolbar) and
  `Ctrl+P` reaches muxal again.

## Editor & tools

- **Code editor pane** — open and edit files in a pane (save / save-as).
- **File browser** — a second, toggleable sidebar (the project row's **files**
  button) showing the project's files as an expandable, gitignore-aware folder
  tree with a search box; click a file to open it in an editor. Resizable; width
  persists. Right-click a row for: copy path, copy relative path, reveal in the OS
  file manager, rename on disk, and open a terminal in that directory.
- **Git marks in the browser** — each row carries its git status: `?` for a file git
  hasn't been told about, `A` staged, `M` modified, `D` deleted, `!` conflicted.
  Folders carry the strongest status beneath them, so a collapsed folder still shows
  that something inside is unadded. Right-click anything with something to stage for
  **Add to git** (a folder stages everything under it).
- **Markdown & image rendering** — `.md`/`.markdown` files render as formatted
  markdown and image files (`png`, `jpg`, `gif`, `webp`, `bmp`, `svg`, …) render as
  images, both by default, with a header **Raw / Rendered** toggle to view the
  source (e.g. an SVG's XML or the markdown text).
- **Rendered-text copy parity** — Ctrl+C and Ctrl+Insert copy the exact selected
  text in rendered Markdown, images, and settings fields without intercepting Ctrl+C
  from a focused terminal.
- **Resource reuse** — opening a local file that already has an editor tab focuses
  that tab and reloads it from disk when its buffer is clean; dirty buffers are
  never overwritten. File links can jump to `#L12C4`, and local HTML links open in
  the browser preview while HTML source panes offer a **Preview** action.
- **External file freshness** — open local text files, including rendered Markdown,
  update within 500 ms when another tool writes them. Refresh keeps cursor and scroll
  position. Dirty buffers show **Reload / Keep mine** instead of being overwritten;
  deleted or unreadable files remain open with a warning. The same behavior works in
  main, secondary, and popped-out editor windows.
- **Resource tab grouping** — files, diffs, and browser pages opened by default join
  the nearest pane of the same type; explicit New Tab and New Pane commands still
  use the requested tab or split placement. Opening another resource as a tab
  preserves the surrounding split geometry, including narrow resource columns.
- **Resize preferences** — project, file/memory, and right-hand tool sidebars keep
  their preferred logical-pixel widths as the window changes size. The center
  splits redistribute proportionally at every level. Temporary minimum-size
  clamps do not save over those widths or proportions; enlarging the window or
  reopening a sidebar restores its preference. Divider dragging updates the
  preference, and double-clicking a center divider makes its split equal.
- **Git diff panel** — a toolbar button (far right) toggles a collapsible
  right-side panel with two tabs:
  - **Files** — the active project's changed files (added / modified / deleted /
    renamed / untracked, color-coded). Click a file to open its diff in a dedicated
    OS window with its own title bar and a **Split / Unified toggle** (remembered):
    **Unified** is a colored diff (green additions / red deletions) whose text is
    selectable + copyable (read-only); **Split** is a side-by-side view (old left /
    new right, aligned, changed rows tinted green/red with line numbers) for quick
    at-a-glance reading. Re-clicking focuses the existing window. Per-file context menu:
    View diff, Stage, Unstage, Discard (with confirmation), Open file. A footer
    commits **all** changes with a message.
  - **Worktrees** — every worktree of the active project, expandable to its changed
    files (same rows + diff windows). Per worktree: **Merge into…** any branch
    (checks it out + merges, then offers to remove the worktree), and **Delete** the
    worktree + its branch (enabled only when no instance is loaded in it).
  Panel width persists per workspace.
- **Git diff pane** — a simpler read-only pane showing the working-tree diff for a
  directory; opens as a **new tab** in the pane it's diffing (from a pane's "View
  changes", the project menu, or a worktree) rather than splitting off a new pane.
- **Command palette / global search** — quick navigation and search across the
  workspace.
- **Find in project** — search within the active project.

## Sidebar & projects

- **Empty-workspace onboarding** — a fresh workspace shows a centered get-started
  screen (the muxal mark, an **Add a project** folder picker, and the
  keyboard-shortcuts chord) in the work area until the first project is added.
- **Project list** — projects with live per-agent status rows; collapse a project.
- **Branch label** — each project row shows its git repo's current branch with a
  branch icon (refreshed live).
- **Project git** — right-click a git project for: git diff (opens the diff pane;
  local projects), switch branch (submenu), new branch, commit, pull, push, fetch,
  and stash / pop / drop stash; each runs off the main thread and toasts its
  result. Destructive actions (switching with a dirty tree, pop/drop stash) ask
  first.
- **Reviewed commit** — the commit dialog lists every changed/untracked file with
  a checkbox (all checked by default); only the checked files are committed, so
  stray files are never swept in. The button shows the count (e.g. *Commit (3)*),
  and a clean tree just toasts “Nothing to commit”.
- **Reorder & rearrange** — reorder projects by dragging a row, or right-click a
  project → **Move up** / **Move down** (disabled at the ends) for an explicit,
  discoverable alternative; the order persists. Swap/move instances between panes
  from the sidebar.
- **Instance names** — muxal persists each program's changing auto-title after a
  short debounce, so restored panes and resume views keep their useful names.
  Custom inline names remain a separate override; clearing one falls back to the
  latest auto-title. Rename opens with the current value selected in a full-width
  editor and commits on Enter or blur without the opening click prematurely
  saving it. Bare session UUIDs and transient startup titles such as `cmd.exe`
  or the agent executable name are never used as display names.
- **Resizable sidebar** — drag to resize (up to half the window); width persists.
- **Fullscreen mode** — `F11` (rebindable) toggles OS fullscreen with the sidebar
  fully hidden. A floating pill at the left edge brings the sidebar back without
  leaving fullscreen; `F11` again exits and restores the previous sidebar state.
- **Multi-monitor** — right-click a project → **Open on display N** to give it a
  full muxal window (toolbar + panes) on that monitor; switch projects and panes
  there like in the main window. It opens with the **sidebar hidden** — the window
  exists to show one project, so the project list starts out of the way; its title
  bar's toggle (or Ctrl+Shift+B) brings the sidebar back for that window alone.
  One window per project: selecting a project that's open elsewhere **raises** its
  window instead of stealing it.
  Every project window's monitor + exact position/size is saved **in the
  workspace**, so reopening the workspace restores each window right where it
  was — dragging a window to another monitor updates its pin, and a
  disconnected monitor keeps the pin for when it returns. **Bring back to
  main window** or closing the window returns the project. Heavy chrome
  (settings, command palette, notification feed) stays in the main window, which
  is raised automatically when needed — but a confirmation about a *pane*
  ("Close terminal?", "Close other tabs?") opens in the window showing that
  pane, and raises it, so the prompt is never stranded on another monitor.
- **No auto-created project** — start empty; add projects via a folder picker.
- **No duplicate projects** — opening a folder that's already a project in this
  workspace is refused with an “already open” error instead of adding a second
  copy. Symlinked or trailing-slash spellings of the same path count as the same
  project.
- **Startup agents** — save the project's open agents as a startup set (preset +
  worktree flag) and relaunch them in one click from the project menu.

## Workspaces & persistence

- **Workspaces** — multiple workspaces, each with its own projects + layout; a startup workspace
  selector.
- **Single instance per workspace** — each workspace is locked while open, so two
  muxal windows can run side by side on **different** workspaces but never the same
  one (which would clobber its layout). Picking a workspace another window already
  holds is refused in the selector with an inline "in use" note; pick a different one
  or close the other window. The lock releases when you switch workspaces or on exit
  (even a crash), so no stale lock blocks the next launch.
- **Full restore** — pane layout, split sizes, window geometry, and sidebar width
  are persisted and restored on launch.

## Settings & theming

- **Settings modal** — sections for Appearance, Editor, Behavior, Agents,
  Runners, Snippets, Loops, Projects, and Keybindings.
- **Themes** — 19 curated choices: Default Light/Dark, Gruvbox Light/Dark,
  Ayu Light/Dark, Catppuccin Latte/Frappe, Alduin, Everforest Dark,
  Fahrenheit, Molokai Dark, Spaceduck, Tokyo Dark, Twilight Grey, Liquid
  Spaceglass, Porcelain, Ember Observatory, and Mainframe. Fahrenheit adds red accents;
  Spaceduck pairs plum surfaces with golden focus accents, orchid, and turquoise;
  Tokyo Dark adds electric blue, ice-cyan active tabs, and sakura-pink syntax on
  deep indigo. Tokyo consolidates the former Night/Storm/Moon variants (saved
  selections migrate automatically). Adventure is removed; saved Adventure
  selections fall back to Default Dark.
- **Mainframe** — black terminal backgrounds with phosphor-green text, neutral
  charcoal chrome, gray labels and borders, and restrained green focus accents.
  Editor syntax stays green with amber literals; ANSI/status colors remain
  distinct for errors, warnings, and other terminal output.
- **Liquid Spaceglass** — a procedural navy/teal/violet space backdrop, sparse
  fixed stars, translucent blue glaze, and fine luminous glass edges in terminal
  panes, with translucent chrome and opaque menus for readability. The effect is
  rendered inside the app, independent of desktop transparency or compositor blur;
  explicit terminal application backgrounds still paint normally.
- **Original palettes** — Porcelain fills the cool-light gap with blue-white
  surfaces, petrol-blue controls, and copper syntax accents. Ember Observatory
  brings warm aubergine surfaces, amber focus accents, and jade strings. Each has
  coordinated editor, terminal ANSI, selection, status, and chrome colors.
- **Keyboard-confirmable dialogs** — every warning/confirmation dialog (close
  tab/pane, quit, pop-out close, delete confirms) takes keyboard focus the
  moment it opens: **Enter confirms, Escape cancels**, no mouse needed. Focus
  is taken only on the open transition, so buttons/checkboxes inside a dialog
  keep their native keyboard behavior.
- **Sizing** — whole-app zoom plus independent UI, terminal, and code/diff font
  sizes, and an adjustable tab-strip height (Settings → Appearance) for the
  tab/control bar on every pane — raising it scales the strip's contents with
  it: tab titles/labels and padding grow continuously, and the strip's buttons
  and icons step up through size classes. The project sidebar reads one notch
  larger than the rest of the chrome (rows, headers, and status badges +2px)
  for legibility.
- **Ambience tint** — a slider in Settings → Appearance (cool −50 … +50 warm)
  casts the whole UI — chrome, panes, and terminal colors — toward warm or cool,
  like a monitor color filter, live as you drag, and it persists across
  restarts. The drift is capped at 15%, so text contrast and semantic colors
  (red = error, ANSI hues) stay intact; at 0 the theme is exactly as authored.
  Paint-level only — identical on every platform, no OS window transparency.
- **Keybindings** — configurable shortcuts with a rebind UI, a cheat-sheet overlay
  (`Ctrl+Shift+/`), `Alt+1–9` to jump to a pane's Nth tab, `Ctrl+1–9` to switch to
  the Nth project, `Ctrl+Alt+1–9` to open a new pane running the Nth agent preset,
  `Ctrl+Shift+G` to toggle the "new agents get a git worktree" switch, `Ctrl+Shift+A`
  to focus the next agent needing attention (blocked, then done), and `Cmd+Q`
  (`Ctrl+Q` elsewhere) to quit from any focus.
- **Behavior** — immediate-save appearance, confirm destructive actions, quit
  confirmation, per-kind close confirmation (terminal on, editor/diff off by
  default), and auto-close a pane when its process exits **cleanly** (an
  abnormal exit always leaves a tombstone pane instead). The terminal
  confirmation is skipped for an untouched **shell** pane — one sitting idle at
  its prompt with no foreground command and no other tabs — since closing it
  loses nothing.
- **Local tmux by default** — "New agents run in a tmux session" defaults **on**
  whenever `tmux` is installed, so panes survive a muxal restart and reattach; the
  toggle greys out and has no effect when tmux isn't found.
- **Agents survive a stray `pkill`** — muxal starts the tmux server itself, from a
  command line naming no project, so an agent running `pkill -f <project>` (to clear
  its own dev server) can't match the *shared* server and kill every session with it.
  Such a `pkill` reaches only that pane's tmux client: the session and the agent keep
  running, and muxal **reattaches the pane automatically** — you see it blink, not die.
  A host has one tmux server shared by every session on it.
- **Killed tmux sessions come back** — if the tmux session (or the whole server) dies
  anyway, the pane doesn't tombstone: muxal recreates the session and relaunches the
  agent with `--resume`, so a resume-capable agent picks its conversation back up where
  it left off (tmux scrollback is the only casualty). A deliberate `tmux kill-session`,
  and an agent simply quitting, still close the pane normally. The feed says which
  happened — *reattached* (session survived) or *session restored* (agent resumed).
- **tmux lifecycle** — closing a **pane** always kills its tmux session. Quitting
  the **app** leaves sessions alive by design (they reattach next launch): when
  any exist, the quit dialog offers an **Also kill local tmux sessions**
  checkbox — off by default; the kill is fire-and-forget, so quitting never waits
  on it.

## Localization

- **Many languages** — the UI auto-detects your OS locale on startup and can be
  switched live from Settings → Appearance → Language (no restart). Any
  untranslated string falls back to English.
- **Translation catalogs** — bundled per-language JSON under `assets/i18n/`,
  (re)generated by `scripts/translate.py`, which drives the `claude` (sonnet) or
  `opencode` CLI in batches of 25 and keeps technical terms / product names (tmux,
  git, worktree, Claude, …) and `{placeholder}` tokens untranslated.
  `python3 scripts/translate.py --check` keeps the catalog in sync with the code.

## Platform & distribution

- **Opt-in focus diagnostics** — the UI profiler correlates GPUI focus-path
  loss with redraw requests and native window ownership. WebView children
  must descend from a registered Muxal window; unrelated WRY apps remain
  external. Records use fixed class buckets and opaque pane/project UUIDs,
  with no terminal text, URLs, titles, or paths.
- **Cross-platform** — Linux (x86_64 + arm64) and macOS (Intel + Apple Silicon).
- **Desktop integration** — app icon and a `.desktop` launcher entry (also the
  notification icon).
- **Host window controls** — muxal defers minimize/maximize/close to the host
  OS's window decorations whenever the window manager provides them (X11, and
  Wayland compositors with `xdg-decoration`), instead of painting a duplicate
  set. On compositors that force client-side decorations (e.g. GNOME Wayland)
  muxal draws its own controls so buttons are never missing. The in-app top bar
  is a single merged row: pane/agent controls (preset, Run task, Loops,
  Snippets, tmux/worktree/restart/close, git-diff) starting at the sidebar
  divider, then Search pushed right alongside the workspace switcher,
  dashboard, notifications, and Settings (outermost). Merging the old separate
  toolbar into the title bar frees a full row of vertical space for panes.
- **Linux: self-cleaning AppImage mounts** — a muxal instance run from an
  AppImage that crashes or is SIGKILLed can't unmount its squashfuse mount, and a
  dead leftover mount makes any filesystem scan (a desktop monitor's periodic
  `df`) stall in the kernel FUSE layer — a periodic Wayland cursor stutter that
  worsens the longer the machine is up. On launch muxal reaps such dead
  `/tmp/.mount_muxal-*` leftovers (leaving live mounts from other running
  instances alone), so they can't accumulate.
- **Packaging & CI** — release packaging per OS/arch on native runners (.deb /
  .rpm / AppImage / .tar.gz for Linux, .dmg / .zip for macOS) and continuous
  integration. The macOS `.dmg` opens to the standard drag-onto-Applications
  layout (the app beside an Applications shortcut). macOS builds are
  Developer-ID-signed + notarized when an Apple cert is configured (else ad-hoc
  signed).
