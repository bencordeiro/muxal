# Licensing

**muxal is GPL-3.0** (see [LICENSE](LICENSE)). This document is a plain-language
map of who wrote what and what that means — it grants no rights beyond the
license itself.

## muxal

muxal is distributed under the **GNU General Public License, version 3**. You
may use, study, modify, and redistribute it under those terms; if you distribute
it — modified or not — the corresponding source must be available under
GPL-3.0. **No commercial or closed-source license for muxal is offered.**

## Upstream: muxel by ProjectHax LLC

muxal is a fork of **muxel**, Copyright ProjectHax LLC, taken under the
**GPL-3.0 option** of that project's dual licensing. Upstream offers muxel
under GPL-3.0 *or* a commercial license from ProjectHax LLC — that commercial
option covers **muxel only**. It does not extend to muxal, and ProjectHax LLC
is not affiliated with and does not endorse this fork. If you need *muxel*
itself outside the GPL terms, that offering is theirs: <https://muxel.sh>.

The `ios/` tree is a verbatim snapshot of upstream muxel's iOS companion
(Copyright ProjectHax LLC, GPL-3.0) and keeps upstream's **App Store
distribution permission** (GPL-3.0 §7 additional permission) — see
[ios/LICENSE](ios/LICENSE). It is not built or shipped by muxal.

## Changes from upstream

GPL-3.0 requires modified versions to be marked as such. muxal's changes from
upstream muxel — the merged command bar, host-native chrome, drag-rearrange
panes, the theme system and Ambience, font verification, and more — are
documented in [FEATURES.md](FEATURES.md) and in this repository's git history.

## Third-party components

- **gpui** — Zed Industries, Apache-2.0
- **alacritty_terminal** — Alacritty contributors, Apache-2.0
- **Some icons in `assets/icons/`** — [Lucide](https://lucide.dev), ISC
- **Bundled themes** — derive from the curated set in upstream muxel (GPL-3.0)

Apache-2.0 and ISC are compatible with GPL-3.0 distribution; their terms
require keeping the copyright and license notices, which this section serves.
