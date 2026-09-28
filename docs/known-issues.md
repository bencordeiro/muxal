# Known issues

Tracked problems and their state. Remove an entry once it's fixed, released,
and verified.

## Terminal glyph spacing / malformed rendering on Arch Linux — FIXED in v0.4.0

- **Status:** fixed — the field fix from the Arch/Omarchy incident report is
  ported into this repo (`crates/muxel-terminal/src/font.rs`), so release
  builds carry it and Arch users never patch their own tree.
- **Root cause:** gpui's `resolve_font` silently substitutes a proportional UI
  face when the requested family (old hardcoded default: DejaVu Sans Mono)
  isn't installed; the terminal then force-spreads every glyph to that face's
  `'m'` advance. Distro images without DejaVu (Arch/Omarchy ship JetBrainsMono
  Nerd Font) hit it.
- **Fix:** the terminal resolves a family that is installed *and* verifiably
  fixed-pitch (advance-width probe), logs a warning when it substitutes, and
  uses installed mono faces only as per-glyph fallbacks. gpui-component's
  global mono family is re-pointed the same way on every theme apply, and
  Settings shows the detected default plus an inline substitution warning.
- **Original report:** `issues1.txt` (local, untracked) §4.
