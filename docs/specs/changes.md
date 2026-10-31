# Changes

## User Stories

1. As a writer, I want the lines changed since the last commit marked beside the text, so that I know what to review after an agent edits a document.
2. As a writer, I want to step to the next and previous change, so that I can review a long document without hunting for the marks.

## Decisions

- Changes are against the file in the git repository's last commit
  (`HEAD`). A file outside a repository, not yet committed, or in a
  repository without commits shows none.
- What is compared is the text as shown, unsaved edits included, so the
  marks follow every edit, the writer's and agents' alike. A commit made
  outside Margin shows when the window is next focused.
- Changes are whole source lines, matched as `git diff --patience` matches
  them, so a deleted paragraph shows as deleted rather than as the
  paragraphs after it changed.
- A changed line has a bar in the left margin beside every line on screen
  that shows part of it, so a one-line paragraph is marked whole, in Show
  Markdown and Reflow Paragraphs too: a thin bar a little way left of the
  text, green for added lines and blue for changed ones. An image or
  diagram is marked along its height.
  Where lines were deleted, a small red triangle sits between the lines
  around them. Lines that show nothing, such as the blank lines between
  paragraphs, have no bar, and a change to only those is marked as a
  deletion is.
- Margin shows neither which words changed nor the old text.
- Next Change and Previous Change move the cursor to the start of the
  nearest change after or before it, wrapping around the document as Next
  Comment does, and scroll it into view. They are unavailable when nothing
  has changed.
- Nothing about changes reaches the CLI.
