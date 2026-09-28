# Margin

A Markdown editor with comment threads, native per platform (GTK 4 /
libadwaita on Linux, AppKit on macOS), plus the `margin` CLI that agents use
to read and answer them. See `README.md` for what it does.

## Specs

- `docs/spec.md`: the product spec, for every platform: problem, user
  stories, and the decisions that don't follow from them.
- `docs/linux/design_system.md`: decisions for the Omarchy (GTK) editor: look,
  colors, fonts, menus, key bindings, storage, lifecycle.
- `docs/macos/design_system.md`: decisions for the macOS (AppKit) editor, built
  and proposed.

Anything that differs by platform belongs in that platform's design system,
not in the spec. Keep the docs in step with behavior changes.

## Layout

A Cargo workspace:

- `crates/core` (`margin-core`): everything platform-independent.
  - `src/md/doc.rs`: analysis of Markdown source (pulldown-cmark offsets):
    which bytes are syntax to hide, line styles, list items, quotes, code
    blocks.
  - `src/md/edit.rs`: editing commands (typing, Enter, Backspace, formatting)
    as pure functions from source + analysis to a `Plan` of byte-range
    changes.
  - `src/comments/`: the thread store (one JSON file per document in the
    data folder's `docs/`, locked read-modify-write) and anchor mapping.
  - `src/diff.rs`: minimal edits between two texts, for applying outside
    changes without moving the cursor or anchors.
- `crates/cli` (`margin`): the binary: agent commands in `src/cli.rs`, and
  opening documents in the editor.
- `crates/gtk` (`margin-gtk`): the GTK editor. `buffer.rs` (TextBuffer
  subclass: routes edits through `md::edit`, restyles changed lines),
  `view.rs` (TextView subclass: draws bullets, checkboxes, quote bars, code
  boxes), `comments.rs` + `card.rs` (the gutter), `window.rs` (files, drafts,
  autosave, watching, menus), `find.rs`, `print.rs`, `settings.rs`,
  `debug.rs` (script driver). Compiles to nothing on macOS.
- `crates/ffi` (`margin-ffi`): the core for Swift, through UniFFI. Positions
  cross as UTF-16 offsets; hot per-keystroke data (spans, lines) as packed
  bytes.
- `macos/`: the AppKit editor. `build.sh` builds `build/Margin.app` (Rust
  static library, generated bindings, `swiftc`, the CLI in
  `Contents/Helpers/`). `DocTextView.swift` (NSTextView on TextKit 1: routes
  edits through the core, hides syntax as null glyphs, draws markers),
  `Styler.swift` (incremental restyling), `Page.swift` (the scrolling page:
  text view beside the gutter view), `Comments.swift` + `Cards.swift` (the
  gutter's cards), `DocumentWindow.swift` (files, drafts, autosave, watching),
  `AppDelegate.swift` (menus, lifecycle), `ScriptDriver.swift` (tests).

## Invariants

- **The buffer text is the file.** Formatting is tags over Markdown source;
  nothing re-serializes Markdown. Edits change only the bytes they must.
- **Editing behavior lives in `md::edit`** and is tested with the `keys()`
  keystroke simulator in its tests. Change behavior there, with a test,
  rather than in GTK handlers.
- **Vertical spacing is only ever space above a line** (`Style::Above`).
  GTK 4.22 aborts ("Byte index N is off the end of the line") when a click
  or `iter_at_location` lands in the space below a line containing invisible
  text. For the same reason the buffer always ends with `\n`, and drawing
  code finds lines with `line_at_y`.
- **GtkTextView cannot remove overlay children.** The comment gutter is one
  `gtk::Fixed` overlay; cards are added to and removed from it.
- **Clicks in the comment gutter stop at the gutter.** GtkButton only claims a
  click on release; without the gutter's event controller the press reaches
  the text view, which moves the cursor and steals focus from cards.
- **Wrapped paragraphs show soft line breaks as spaces in the buffer** (same
  length, so offsets match), remembered in `State::soft`. `State::text` and
  `text_string()` are always the file text; never read the file text from the
  GTK buffer.
- **The look is Omarchy's: square, flat, muted.** No radii, shadows or
  avatars; 1px muted borders, the accent only on what has focus. Rarely used
  commands go in the menu, not the header. Omarchy's rounding setting only
  reaches windows and its shell, so this is hard-coded, as in Omarchy's own
  apps.
- **Comments have no author.** One person comments and agents reply; a
  thread is its comment followed by replies. Old stores' `author` and
  `resolved_by` fields are ignored on load and dropped on the next write.
- **Comment anchors are text marks** in the editor and byte ranges against a
  stored snapshot in the store; `Comments::sync` maps them across edits made
  while the editor was closed.

## Testing

`cargo test` covers analysis, editing, store and anchors. For the UI, run the
editor on a Broadway display with `MARGIN_SCRIPT` (see `README.md`); take
screenshots with the `shot` step and read them. Set `MARGIN_DATA_DIR` so test
comments stay out of the real store. Anything a mouse does must be tested
with real clicks (`tools/run-ui-script.sh`, see `crates/gtk/src/debug.rs`): scripted
steps bypass GTK's event routing, where click bugs live.

On macOS, `macos/tests/run.sh` runs the UI scripts in `macos/tests/` against
a test build (`macos/build.sh debug`; release builds have no script driver)
and diffs their output; add a script there for new behavior. Edits of every
kind must reach `DocTextView.shouldChangeText(inRanges:…)` or an action
override, so they go through the core; the cursor is kept out of hidden
syntax in the selection delegate method, not in movement overrides.
TextKit 1 attaches the null glyphs of a hidden line prefix to the previous
line's fragment, so a line whose prefix is hidden starts mid-paragraph:
`HidingLayoutDelegate` collapses fully hidden fragments and adds the space
above such lines itself. Find a line's position through a visible character
(its content start or newline), not its first character.
