# Margin

A GTK 4 / libadwaita Markdown editor with comment threads, plus the `margin`
CLI that agents use to read and answer them. See `README.md` for what it does.

## Layout

A Cargo workspace:

- `crates/core` (`margin-core`): everything platform-independent.
  - `src/md/doc.rs`: analysis of Markdown source (pulldown-cmark offsets):
    which bytes are syntax to hide, line styles, list items, quotes, code
    blocks.
  - `src/md/edit.rs`: editing commands (typing, Enter, Backspace, formatting)
    as pure functions from source + analysis to a `Plan` of byte-range
    changes.
  - `src/comments/`: the thread store (one JSON file per document under
    `$XDG_DATA_HOME/margin/docs`, locked read-modify-write) and anchor
    mapping.
- `crates/cli` (`margin`): the binary: agent commands in `src/cli.rs`, and
  opening documents in the editor.
- `crates/gtk` (`margin-gtk`): the GTK editor. `buffer.rs` (TextBuffer
  subclass: routes edits through `md::edit`, restyles changed lines),
  `view.rs` (TextView subclass: draws bullets, checkboxes, quote bars, code
  boxes), `comments.rs` + `card.rs` (the gutter), `window.rs` (files, drafts,
  autosave, watching, menus), `find.rs`, `print.rs`, `settings.rs`,
  `debug.rs` (script driver).

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
