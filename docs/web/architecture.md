# Web architecture

The web app's architecturally significant decisions: the ones costly to
change later, e.g. its language and frameworks. Those every platform shares
are in `docs/architecture.md`. The web app isn't built yet.

- The editor is CodeMirror 6. Its document is plain text, so the text the
  writer edits is the file's, as in the native editors.
- Hidden syntax and visual components are CodeMirror decorations (hidden
  and replaced ranges, block widgets, ranges the cursor skips), made from
  the core's account of what the document shows.
- Not ProseMirror or the editors built on it (Tiptap, Milkdown), nor
  Lexical: they parse Markdown into a tree of their own and write it back,
  which rewrites the file.
- The core runs in the browser, compiled to WebAssembly.
