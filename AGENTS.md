# Margin

A Markdown editor with comment threads, native per platform, plus the
`margin` CLI that agents use to read and answer them. See `README.md` for what
it does.

## Specs

This project uses specification-driven development.

- `docs/spec.md`: the product spec: problem, user stories, product
  decisions, and the UX the platforms share.
- `docs/<platform>/design_system.md` (`linux`, `macos`, `web`): a platform's
  UX.
- `docs/architecture.md`: the architecturally significant decisions every
  platform shares, the ones costly to change later, e.g. languages and
  frameworks.
- `docs/<platform>/architecture.md` (`linux`, `macos`, `web`): a
  platform's own architecturally significant decisions.

UX that holds on every platform belongs in the spec, and architecture that
holds on every platform in `docs/architecture.md`; a platform's documents
hold only what is specific to it. How the code does what the specs say
belongs in the code and its comments.
Keep the specs in step with user-visible behavior changes. Keep `margin --help` and
`skills/margin/SKILL.md` in step with agent-facing behavior changes.

## Layout

Before changing code, read the `AGENTS.md` named beside its directory: how
to build, run and test it.

- `crates/` (`crates/AGENTS.md`): the Rust crates.
  - `core`: everything platform-independent.
  - `cli`: the `margin` binary.
  - `gtk` (`crates/gtk/AGENTS.md`): the Linux editor.
  - `ffi`: the core for Swift, through UniFFI.
- `macos/` (`macos/AGENTS.md`): the macOS editor.

## Launching

To launch Margin, run this checkout's build, not an installed one: the user
wants to test the latest changes. On macOS, `macos/tools/run.sh FILE…` builds
it and returns once the files are shown.
