# Theme collection review

The collection had 19 public choices before this review. Default Dark remains
the fresh-install default; these additions do not change saved selections.

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

## Workbench Dark

The missing familiar editor family was Visual Studio / VS Code Dark+: neutral
graphite with blue keywords, jade types, warm orange strings, pale-gold functions,
and sky-blue properties. Workbench Dark is an original Muxal palette using that
color vocabulary, with layered chrome and readable terminal/status colors.

Reference: Microsoft's official
[Dark+ syntax palette](https://github.com/microsoft/vscode/blob/main/extensions/theme-defaults/themes/dark_plus.json)
and [Dark Visual Studio palette](https://github.com/microsoft/vscode/blob/main/extensions/theme-defaults/themes/dark_vs.json).
The theme JSON is authored here; no theme files or branding assets were copied.

## Clay Dark

The user also requested a theme close to Claude. Clay Dark pairs warm charcoal
surfaces with ivory text and the terracotta accent `#D97757`, with restrained sage,
blue, and sand for syntax and terminal colors. It retains Muxal's normal fonts
and layout. The original palette is inspired by the colors in
[Anthropic's published brand guidelines](https://github.com/anthropics/skills/blob/main/skills/brand-guidelines/SKILL.md);
it is an optional Muxal theme, not an official Anthropic product.

Both additions use the existing theme loader and ambient tint controls. The
collection now has 21 public choices. Palette checks cover body text, secondary
labels, primary buttons, and syntax contrast; native review should assess chrome,
active/inactive tabs, selection, terminal ANSI colors, editor highlighting, and
Spider status/glow visibility.
