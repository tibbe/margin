# Linux architecture

The Linux editor's architecturally significant decisions: the ones costly to
change later, e.g. its language and frameworks. Those every platform shares
are in `docs/architecture.md`.

- The editor is written in Rust, with GTK 4 (4.20+) and libadwaita (1.8+)
  through their Rust bindings, and is part of the `margin` binary.
- The document is one `GtkTextView` whose buffer holds the file's text.
  Hidden syntax is an invisible tag.
- Visual components (code boxes, tables, images, diagrams) are drawn in the
  text view's snapshot, in space its layout leaves for them, over their
  hidden source. Not as child widgets or paintables: both insert a
  character into the buffer, which then isn't the file, and GTK's undo
  doesn't record them.
- The UI is tested headlessly, on a Broadway display, with a script that
  types, presses keys, comments and takes screenshots.
- Distributed as source: `install.sh` builds `margin` and installs it into
  `~/.local/bin`.
