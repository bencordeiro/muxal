# Contributing to muxal

Thanks for your interest in muxal!

## License of contributions (please read)

muxal is **GPL-3.0** (see [LICENSING.md](LICENSING.md)) — there is no second
license. **By submitting a contribution** — a pull request, patch, or any code,
docs, or other material — **to this project, you agree that:**

1. You are the author of the contribution (or have permission from the rights
   holder to submit it), and to your knowledge it does not violate any third
   party's rights.
2. You license your contribution under the **GPL-3.0**, the same license as the
   rest of the project, so it can be distributed together with muxal.

If you can't agree to that, please don't submit a contribution.

## Development

Build and run instructions are in the [README](README.md). Before opening a PR,
run the full gate and fix everything it reports:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings   # warnings are errors
cargo test --workspace
cargo build -p muxal
```

`AGENTS.md` documents the workspace layout and project conventions. Keep pure,
testable logic in `muxal-core`, and add a `FEATURES.md` entry when you add or
change a user-facing feature.
