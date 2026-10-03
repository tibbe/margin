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
- **The text is the file.** An editor's model of a document is the file's
  text. Nothing is parsed into a document model and written back: an edit
  changes only the bytes it must, comments anchor to ranges of the file's
  text, and every position is a position in the file.
- **Anchors are on characters.** An anchor is on its text's first and last
  characters, with both edges closed, and moves as they do, whether the
  edits are typed or worked out from an outside change. These are the
  positions every text CRDT (Yjs, Loro, Automerge) has, so a collaborative
  store could keep anchors as its positions without changing where they go.
- **What is shown is a projection of the source: the core's display map.**
  Hidden syntax, list markers, code boxes, tables, images and diagrams are
  drawn over, or in place of, the source they stand for, and reflowing
  shows a paragraph's soft line breaks as spaces. The core projects the
  text into shown paragraphs, as CodeMirror's decorations and Zed's
  display map do. It decides, for each range of the text:
  - whether it shows as text, is hidden, shows as other text (a soft line
    break while reflowing, a table cell's gap), shows as an object (an
    image, a diagram), or shows on a line laid out as nothing (a blank
    line, a fence);
  - when the cursor reveals its source;
  - where the cursor may stop: never in hidden syntax, at the left edge of
    inline syntax, past a line's block syntax;
  - the text as shown, for find.

  Paragraphs are those shown: a reflowed paragraph is one, though the file
  breaks it into lines. The map translates positions both ways, so each
  editor lays out and draws the shown paragraphs, and finds where the
  cursor, the selection, highlights, cards and change bars go, through it.
  A new kind of block is one rule in the core and one renderer per editor.
- **Diagrams are drawn in the core.** Mermaid diagrams are rendered by
  merman, a Rust port of mermaid.js, and rasterized with resvg. So every
  editor shows the same picture, without a browser. merman is pinned to one
  version, and built without its optional ELK layout, which is under the
  copyleft EPL-2.0 license. merman is alpha: its pictures can
  differ from mermaid.js's, and a diagram it can't draw shows as an error.
