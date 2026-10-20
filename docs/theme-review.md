# Theme collection review

The collection had 19 public choices before this review. Default Dark remains
the fresh-install default; the added theme does not change saved selections.

| Theme | Character |
| --- | --- |
| Default Light | Neutral, straightforward light interface |
| Default Dark | Balanced charcoal, soft light text, pastel blue focus |
| Alduin | Muted charcoal, brown, and sage |
| Ayu Light | Off-white, cyan, and orange |
| Ayu Dark | Deep navy, orange, and cyan |
| Catppuccin Latte | Cool light surfaces and pastel blue/purple |
| Catppuccin Frappe | Blue-gray surfaces and rose/lavender pastels |
| Ember Observatory | Warm plum, amber, mauve, and sage |
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
