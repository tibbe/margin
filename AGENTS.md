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

Anything that differs by platform belongs in that platform's design system,
not in the spec. Keep the specs, `margin --help` and
`skills/margin/SKILL.md` in step with behavior changes.

## Layout

- `crates/core`: everything platform-independent.
- `crates/cli`: the `margin` binary.
- `crates/gtk`: the Linux editor.
- `crates/ffi`: the core for Swift, through UniFFI.
- `macos/`: the macOS editor.

## Testing

`cargo test` covers the core.
