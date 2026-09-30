# Margin

A Markdown editor with comment threads, native per platform, plus the
`margin` CLI that agents use to read and answer them. See `README.md` for what
it does.

## Specs

- `docs/spec.md`: the product spec, for every platform: problem, user
  stories, and product decisions.
- `docs/linux/design_system.md`: the UX design spec for the Linux (GTK)
  editor.
- `docs/macos/design_system.md`: the UX design spec for the macOS (AppKit)
  editor.

Platform-specific UX decisions belong in that platform’s design system.
Keep the specs in step with user-visible behavior changes. Keep `margin --help` and
`skills/margin/SKILL.md` in step with agent-facing behavior changes.

## Layout

- `crates/core`: everything platform-independent.
- `crates/cli`: the `margin` binary.
- `crates/gtk`: the Linux editor.
- `crates/ffi`: the core for Swift, through UniFFI.
- `macos/`: the macOS editor.

## Checks

Before committing or handing work back, run the checks for what you
changed. Warnings count as failures.

- Rust: `cargo fmt --all`, then
  `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --all-targets` and
  `cargo test --workspace`, which covers the core. After changing doc
  comments, also `CARGO_BUILD_WARNINGS=deny cargo doc --workspace --no-deps`.
- On macOS, `margin-gtk` compiles to nothing, so the Rust checks skip it.
  Changes to it, or to the core API it uses, need the checks on a Linux
  machine; say so when you hand them back unchecked.
- The macOS app: the checks in `macos/AGENTS.md`, also after changing
  `crates/ffi` or the core API it uses.
