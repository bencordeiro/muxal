# Plan: Easy updates (install/update script + Settings check)

Status: **planned, not started** — open questions at the bottom need answers
before any code. Both features touch *installs* and *the network*, so the test
story is half the design.

## Guiding principles

- **Never brick an install**: download → verify → *then* atomic swap
  (copy-then-rename, the ETXTBSY-safe pattern we already own). Failure at any
  step leaves the old binary running.
- **Never execute unverified bytes**: releases ship `SHA256SUMS.txt` (new), the
  script verifies before install.
- **Nothing touches the real system in testing**: every path is overridable and
  every test runs in a throwaway `mktemp` sandbox with a *fake release server*.
- **No silent network**: the app only ever checks on an explicit click.

## Feature A — `scripts/get.sh` (install + update in one)

- **Entry point**:
  `curl -fsSL https://raw.githubusercontent.com/bencordeiro/muxal/master/scripts/get.sh | sh`
  (also works downloaded-and-inspected; script supports `--check` dry-run).
- **Detects**: OS (Linux/macOS), arch (x86_64/aarch64), install mode:
  - `dpkg` present → `.deb` · `rpm`/`dnf` present → `.rpm` · else → `tar.gz`
    into `~/.local/bin` (+ `.desktop`/icon, `install-desktop.sh` logic inline)
  - `--system` for `/usr` paths, `--appimage` opt-in, macOS → universal `.zip`
    → `/Applications` (Gatekeeper note printed)
- **Update flow**: compare installed vs latest → prints `0.4.2 → 0.4.3` →
  download to tmp → **sha256 verify** → atomic swap → keep `muxal.bak` →
  re-register launcher. Refuses downgrades without `--force`.
- **Depends on `muxal --version`** (new tiny flag: print + exit before GUI
  init) so the script can ask the binary its version safely — good hygiene
  regardless.
- **Overrides for testing**: `MUXAL_RELEASE_BASE_URL`, `MUXAL_LATEST_API`,
  `MUXAL_BIN_DIR`, `MUXAL_FAKE_OS/ARCH` — same isolation philosophy as
  `scripts/dev.sh`.

## Feature B — Settings "Check for updates"

- **UX**: next to the version label (bottom-left of Settings):
  `[Check for updates]` → states: `checking…` / `up to date (v0.4.3)` /
  `v0.4.4 available → [Open releases]` / `couldn't check`. Click-only; never
  auto-checks (privacy + GitHub's 60/hr unauthenticated rate limit).
- **Logic (pure, in `muxal-core`)**: `parse_latest_tag(json) -> Option<String>`
  + `is_newer(latest, current)` semver compare — fully unit-tested (incl.
  `0.4.10 > 0.4.3`, prerelease tags).
- **Transport decision** (open question): add tiny `ureq` (rustls) **or** shell
  out to `curl` (present on every supported platform). Never downloads anything
  — GET one API URL, show a link. Respects the fork's deliberate "no in-app
  updater" removal.

## Release workflow addition

- Release job uploads **`SHA256SUMS.txt`** (hashes of all assets) — the trust
  anchor for Feature A.

## Testing strategy (the safe/isolated part)

**Script** — `scripts/test-get.sh` self-test, run in CI's Linux job too:

1. Fake release: a local "release" (dummy assets + checksums) served by
   `python3 -m http.server`, pointed at via `MUXAL_RELEASE_BASE_URL`.
2. Matrix: fresh install · update (asserts `old → new` + `.bak`) · **corrupt
   checksum → refuses** · wrong arch → clean error · binary held open by a
   process → swap still succeeds (rename) · server down → old install untouched
   · `--check` reports without changing anything.
3. All against `HOME=$(mktemp -d)` + `MUXAL_BIN_DIR` sandbox — the real
   `~/.local/bin` and system paths are never addressable in tests.

**App-side** — pure compare/parse tests in `muxal-core`; transport behind a tiny
injectable seam (tests feed canned JSON, no network); GUI states extracted as a
pure state machine (repo rule: decision logic in core); then a human check in
`scripts/dev.sh` for the button's look.

## Phases

0. This plan → sign-off on the open questions.
1. `muxal --version` + version-compare/JSON-parse (core + tests) +
   `SHA256SUMS.txt` in workflow.
2. Settings check (transport + UI + states) → verified in dev.
3. `scripts/get.sh` + `scripts/test-get.sh` + CI hook → sandbox matrix green.
4. Docs (README "Update" section, FEATURES.md) → ship in the next release.

## Open questions (need answers before Phase 1)

1. **Transport**: `ureq` dep (clean, testable) vs shell out to `curl` (zero
   deps)? *Lean: ureq.*
2. **Script default install scope**: user (`~/.local/bin`) default with
   `--system` opt-in — or auto (root → system)? *Lean: user-default.*
3. **macOS in v1?** (zip → /Applications, ad-hoc signing caveats) or
   Linux-first?
4. **Downgrades**: refuse unless `--force` — agreed?
5. **Keep `muxal.bak`** on every update — or overwrite clean?
6. **Script name**: `get.sh` (recommended; avoids colliding with the existing
   dev-oriented `scripts/install.sh`).
