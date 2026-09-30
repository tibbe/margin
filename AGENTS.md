# Margin

A Markdown editor with comment threads, native per platform, plus the
`margin` CLI that agents use to read and answer them. See `README.md` for what
it does.

## Specs

- `docs/spec.md`: the product spec: problem, user stories, product
  decisions, and the UX the platforms share.
- `docs/linux/design_system.md`: the Linux (GTK) editor's design system.
- `docs/macos/design_system.md`: the macOS (AppKit) editor's design system.

UX that holds on every platform belongs in the spec; a design system holds
only the UX specific to its platform.
Keep the specs in step with user-visible behavior changes. Keep `margin --help` and
`skills/margin/SKILL.md` in step with agent-facing behavior changes.

## Layout

- `crates/core`: everything platform-independent.
- `crates/cli`: the `margin` binary.
- `crates/gtk`: the Linux editor.
- `crates/ffi`: the core for Swift, through UniFFI.
- `macos/`: the macOS editor.
