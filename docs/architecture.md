# Architecture

Margin's architecturally significant decisions that hold on every platform:
the ones costly to change later. Each platform's own are in
`docs/<platform>/architecture.md`.

- **One core, in Rust.** Everything platform-independent is in the core:
  - Markdown analysis and the editing rules;
  - find;
  - the comment store and anchoring;
  - keeping the text and its file in step (autosave, outside edits,
    conflicts);
  - the changes since the last commit;
  - what the document shows (below).

  The `margin` CLI is built on it. Each editor provides the UI and system
  integration, and routes every edit through the core. The Linux editor
  calls it from Rust, the macOS editor through UniFFI, and the web app runs
  it compiled to WebAssembly.
- **The text is the file.** Each editor shows a document in one text view
  whose text is exactly the file's. Nothing is parsed into a document model
  and written back. So an edit changes only the bytes it must, comments
  anchor to ranges of the file's text, and positions in the file map to
  positions on screen, for cards, find and change bars.
- **What is shown is a view of the source.** Hidden syntax, list markers,
  code boxes, tables, images and diagrams are drawn over, or in place of,
  the source they stand for. The core decides, for each range of the text:
  - whether it shows as text, is hidden, is drawn in place at text size (a
    checkbox), or shows as a block, such as a table, an image or a diagram,
    with what is needed to draw it;
  - when the cursor reveals its source;
  - where the cursor may stop;
  - the text as shown, for find.

  Each editor measures, lays out and draws. A new kind of block is one rule
  in the core and one renderer per editor.
- **Diagrams are drawn in the core.** Mermaid diagrams are rendered by
  merman, a Rust port of mermaid.js, and rasterized with resvg. So every
  editor shows the same picture, without a browser. merman is pinned to one
  version, and built without its optional ELK layout, which is under the
  copyleft EPL-2.0 license. merman is alpha: its pictures can
  differ from mermaid.js's, and a diagram it can't draw shows as an error.
