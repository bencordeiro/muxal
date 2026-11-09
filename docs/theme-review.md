# Theme collection review

The collection had 19 public choices before this review. Default Dark remains
the fresh-install default; the added theme does not change saved selections.

| Theme | Character |
| --- | --- |
| Default Light | Neutral, straightforward light interface |
| Default Dark | Balanced charcoal, soft light text, pastel blue focus |
| Alduin | Muted charcoal, brown, and sage |
| Ayu Light | Off-white, cyan, and orange |
| Ayu Dark | Ink terminals, slate-blue frame, citron focus, orange and blue syntax |
| Catppuccin Latte | Cool light surfaces and pastel blue/purple |
| Catppuccin Frappe | Blue-gray surfaces and rose/lavender pastels |
| Ember Observatory | Night-sky terminal, indigo-violet frame, coal top bar, ember focus |
| Everforest Dark | Green-gray surfaces and earthy peach/sage |
| Fahrenheit | Near-black with red accents |
| Gruvbox Light | Cream, rust, olive, and earthy colors |
| Gruvbox Dark | Dark earth tones, gold, peach, and sage |
| Liquid Spaceglass | Procedural navy backdrop, teal/violet, translucent chrome |
| Mainframe | Black, phosphor green, and amber literals |
| Molokai Dark | Graphite with vivid pink accents |
| Porcelain | Blue-white, slate, and teal |
| Spaceduck | Plum, gold, orchid, and turquoise |
| Tokyo Dark | Deep indigo, electric blue, cyan, and pink |
| Twilight Grey | Neutral dark surfaces and muted gold/sage |

## Classic editor comparison

A Workbench Dark candidate used the familiar Visual Studio / VS Code color
vocabulary. The user found its overall appearance too similar to Default Dark
and Alduin, so it was removed after review. The existing collection already
covers that neutral editor appearance; another near-duplicate adds little.

## Clay Dark

The user also requested a theme close to Claude. Clay Dark pairs warm charcoal
surfaces with ivory text and the terracotta accent `#D97757`, with restrained sage,
blue, and sand for syntax and terminal colors. It retains Muxal's normal fonts
and layout. After user review, its dark surfaces were lifted about 2% toward white;
Fahrenheit received the same small lift without changing its accent or text colors.
The original palette is inspired by the colors in
[Anthropic's published brand guidelines](https://github.com/anthropics/skills/blob/main/skills/brand-guidelines/SKILL.md);
it is an optional Muxal theme, not an official Anthropic product.

Clay Dark uses the existing theme loader and ambient tint controls. The
collection now has 20 public choices. Palette checks cover body text, secondary
labels, primary buttons, and syntax contrast; native review should assess chrome,
active/inactive tabs, selection, terminal ANSI colors, editor highlighting, and
Spider status/glow visibility.

## Mainframe rework and three additions

The collection now has 24 public choices. An audit of the existing palettes found
that agent status comes from `primary`, `warning`, and `success`, so a palette
whose accent matches one of those loses a status distinction. The pane header
also uses `title_bar` when focused and `secondary` when not, so these four themes
set `secondary` below `title_bar` and give buttons their own
`button.secondary.*` values.

| Theme | Reason to exist |
| --- | --- |
| Mainframe (reworked) | Hacker green on neutral surfaces: green is reserved for text and accents |
| Verdigris | Only saturated deep-teal surfaces; hot copper is teal's complement |
| Rosewood | Red-brown wood frame and oxblood terminals with a cool teal focused header, mirroring Verdigris |
| Ledger | Warm paper chrome with dark ink terminals and banker's green |
| Ledger Dark | Green-bound top bar, aged parchment-toned surfaces, ink terminals |

Mainframe's green terminal text now comes from its terminal zone, and its `done`
status is cyan because `working` is green. A first rework also tinted its surfaces
and interface text green; the user found that too green on green, so surfaces and
labels returned to neutral.
Verdigris was pushed away from Everforest Dark after a first version measured too
close in both surface and accent. Each palette was checked for text, status,
ANSI, syntax, selection, and surface separation; native review remains the owner's.

## Theme zones

An audit found that every theme looked painted with one brush: the `background`
token drove the terminal, active tabs, the gaps behind panes, and most dialogs,
and the terminal's colors were derived from the app's. Muxal read only about 17
colors for its own interface, and never read the schema's tab colors.

A theme may now add a `muxal` object beside `colors` and `highlight`, setting
`floor`, `divider`, focused and idle pane headers, `idle_border`, a `terminal`
palette (background, foreground, cursor, selection, sixteen ANSI colors), and
`status` text for soft badges. Each value is optional and falls back to its
previous color, so themes without the section are unchanged. `background` remains
the surface for documents and dialogs. These colors are read in `theme.rs` and
follow the ambience tint. No layout or behavior changes.

Verdigris, Mainframe, Clay Dark, Ledger, Ledger Dark, Rosewood, Ember Observatory,
and Ayu Dark use zones. Ledger Dark exists because the owner liked Ledger's
paper-and-ink contrast but avoids light interfaces. Dialogs, more text levels, and
border levels remain possible follow-ups.

Three themes were reworked because they measured as near-duplicates. Rosewood and
Ember shared almost the same surfaces; Ember also nearly matched Spaceduck, and its
working and input statuses were the same color. Ayu Dark's background matched
Mainframe's and Twilight Grey's while its blue accent repeated Tokyo and Default
Dark. A gold accent for Ayu was rejected because Ember, Spaceduck, Gruvbox, and
Twilight already use one; citron comes from Ayu's own lime and no other theme uses it.
