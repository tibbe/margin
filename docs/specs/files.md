# Files

## User Stories

1. As a writer, I want saving to change only the bytes I edited, so that diffs show only real changes.
2. As a writer, I want documents to save themselves, so that I never think about saving.
3. As a writer, I want to be asked before closing only when text would be lost, so that closing is quick but never destructive.
4. As a writer, I want untitled documents kept until I name them, and brought back after a crash, so that drafts are never lost.
5. As a writer, I want comments to move with a document I save under a new name, so that renaming doesn't lose my review.
6. As a writer, I want an agent's edits to an open document picked up, and merged with mine when they don't overlap, so that we can both work at once.
7. As a writer, I want to choose which version to keep when our edits overlap, so that nothing is lost without my say.

## Decisions

- UTF-8 only; other files are refused. A missing path opens empty and is
  created on first save.
- Documents are identified by canonical path; one window per document.
- CRLF files are written back as CRLF; a missing final newline is added.
- Documents save shortly after the last change, on focus loss and on close.
  Saves never leave a half-written file and keep the file's permissions.
- Untitled documents are kept as drafts until saved, and come back after a
  crash or relaunch. Launched without files, the editor reopens them, or
  with none asks for a file.
- Closing asks only for an untitled document with text, or one whose save
  failed. Empty untitled documents are discarded silently.
- Outside changes: reload if there are no unsaved edits; otherwise a
  three-way line merge against the last saved text, applied and saved when
  clean; on overlap, ask Keep My Version or Load Disk Version. Announce
  which happened ("Updated from disk").
- Comments are keyed by path. Save As moves them and drops any stored for a
  file it overwrites; moving a file outside Margin loses them.
