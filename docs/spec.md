
# Margin product spec

Margin is a Markdown editor for one person working with coding agents, and
the `margin` CLI those agents use to read and answer the person's comments.

## Problem Statement

I work with coding agents through Markdown documents: the agent writes a plan
or spec, I review it, we go back and forth, and the agent builds from it.
Reviewing is the slow part.

Text editors show Markdown as source, which is tiring to read as prose. Rich
editors hide the syntax but rewrite the file on save (re-wrapping, changing
list markers, escaping), so every save makes a noisy diff. And there is no
good way to give feedback on a passage: agents can't read Google Docs
comments, comments written into the file pollute it, and pasting fragments
into a chat loses where they came from. The agent's answers end up in the
chat, away from the text, and its edits to the document are hard to follow.

## Solution

A native editor that reads like a word processor and writes like a text
editor, with Google Docs-style comments in the margin that agents read and
answer through a CLI.

- **The file is the document.** The editor edits the Markdown text directly
  and never re-serializes it; an edit changes only the bytes it must.
- **Markdown is an input method.** Typed syntax formats as you type and is
  hidden; the screen reads like a document, in proportional type.
- **Keys behave like a word processor's**, expressed as minimal changes to
  the source.
- **Comments live beside the file, never in it**, and stay attached to their
  text as anyone edits it. One person and their agents comment and reply,
  and each message says which of them wrote it.
- **Agents are first-class** through the `margin` CLI, like `tuicr` and
  `hunk`. Their edits and replies appear in the open editor as they happen.
- **Quiet and native.** Documents save themselves; rarely used commands live
  in menus; each platform's editor behaves like that platform's apps.

## User Stories

The actors are the **writer** (the one person who comments) and a **coding
agent**.

1. As a writer, I want saving to change only the bytes I edited, so that diffs show only real changes.
2. As a writer, I want HTML, front matter and link reference definitions shown as dimmed source, and tables as grids that edit as text, so that nothing in the file is hidden or misrendered.
3. As a writer, I want images shown as their alt text, so that I know an image is there without the editor loading files.
4. As a writer, I want the syntax of the blank line I'm on, and the fences of the code block I'm in, shown, so that I can edit what is otherwise invisible.
5. As a writer, I want a marker without its space (`#`, `-`) left as typed, so that a line isn't formatted before I mean it.
6. As a writer, I want to switch a window to raw source, keeping my place, so that I can see exactly what is in the file.
7. As a writer, I want hard-wrapped paragraphs to reflow to the window when I choose, without changing the file, so that I can read them comfortably.
8. As a writer, I want Enter to start a new paragraph and Shift+Enter to break the line within it, so that I get the break I mean.
9. As a writer, I want Enter in a list, quote or code block to continue it, and Enter on an empty item to leave the list, so that I never type markers.
10. As a writer, I want numbered lists renumbered only when they are numbered in sequence, so that `1.` `1.` `1.` lists stay that way.
11. As a writer, I want Backspace at the start of a heading, quote or item to turn it back into a paragraph, so that undoing a block type is one key.
12. As a writer, I want deleting the last character of formatted text to remove its markers, so that no empty `****` is left behind.
13. As a writer, I want splitting or partly un-formatting a phrase to leave well-formed Markdown on both sides, so that I never see stray markers.
14. As a writer, I want typing at the end of bold text to continue the bold only when the cursor is inside it, and never to extend a link, so that formatting starts and stops where I expect.
15. As a writer, I want copying to give balanced Markdown and pasting to insert Markdown, so that formatting survives the clipboard.
16. As a writer, I want the characters I type written exactly as typed, so that the file holds what I typed.
17. As a writer, I want search to match the text as shown, so that "bold text" finds `**bold** text`.
18. As a writer, I want replacing to keep formatting around a match, so that I can rename things safely.
19. As a writer, I want documents to save themselves, so that I never think about saving.
20. As a writer, I want to be asked before closing only when text would be lost, so that closing is quick but never destructive.
21. As a writer, I want untitled documents kept until I name them, and brought back after a crash, so that drafts are never lost.
22. As a writer, I want comments to move with a document I save under a new name, so that renaming doesn't lose my review.
23. As a writer, I want an agent's edits to an open document picked up, and merged with mine when they don't overlap, so that we can both work at once.
24. As a writer, I want to choose which version to keep when our edits overlap, so that nothing is lost without my say.
25. As a writer, I want to select text and comment on it, so that feedback is attached to exactly the passage it is about.
26. As a writer, I want each thread's card beside its text in the margin, so that I read comments alongside the document.
27. As a writer, I want the cursor entering commented text to focus its thread, so that moving through the text moves through the review.
28. As a writer, I want to edit or delete any comment or reply, so that I can fix what I wrote or clear what no longer helps.
29. As a writer, I want long comments and replies cut short until I ask for the rest, so that one long message doesn't push the other threads out of view.
30. As a writer, I want each message to say whether I or an agent wrote it, so that I can follow who said what in a thread.
31. As a writer, I want resolving, resolving all, editing and deleting to be undoable, so that a slip doesn't lose a thread.
32. As a writer, I want resolved threads hidden unless I ask for them, so that the margin shows what is still open.
33. As a writer, I want a thread whose text was deleted to stay, showing what it was about, so that feedback is never silently lost.
34. As a writer, I want a thread to follow its text when it is reworded, so that a comment on "blue/green" follows the change to "canary".
35. As a writer, I want to be told when an agent adds, answers, resolves, reopens or deletes threads, even while I am in another app, so that I notice its answers when it replies asynchronously.
36. As a writer, I want to copy the open comments as a list with locations, so that I can paste a review into an agent's chat.
37. As a coding agent, I want the open threads with `file:line:column` locations against the file as it is now, so that I can find what was asked even after my own edits.
38. As a coding agent, I want to find documents with open threads under the current directory, so that I don't need to be told the file.
39. As a coding agent, I want a thread's quote as the file's exact Markdown source, so that I can find and edit the text.
40. As a coding agent, I want to reply, resolve and reopen, so that I report what I did where the writer will see it.
41. As a coding agent, I want to start threads on quoted text, with ambiguous quotes refused, so that my question lands on the right text.
42. As a coding agent, I want to open a document for review and return at once, so that my shell isn't blocked.
43. As a coding agent, I want JSON output, so that I can process threads reliably.
44. As a coding agent, I want the same commands and output on every platform, so that one skill works everywhere.

## Decisions

### Files

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
  clean; on overlap, ask Keep My Version or Load Disk Version. Say which
  happened.
- Comments are keyed by path. Save As moves them and drops any stored for a
  file it overwrites; moving a file outside Margin loses them.

### Rendering

- CommonMark with GitHub tables, strikethrough and task lists, and YAML front
  matter.
- Tables show as grids: the `|`s, the cells' padding and the delimiter row
  are hidden, columns take their widest cell's width and the delimiter row's
  alignment, and the header row is bold. A row stays on one line. Where a
  platform does not draw grids yet (see its design system), tables show as
  source like HTML.
- HTML, front matter and link reference definitions show as dimmed
  monospace source; images as italic, link-colored alt text; bare URLs and
  autolinks as links with the URL visible. Checked tasks are struck through
  and dimmed; Heading 6 is dimmed.
- Bullets change shape with nesting depth; a code block shows its language.
- The text column holds about 100 characters, with the comment gutter to its
  right. Spacing and sizes are proportional to the text size. Zoom is shared by all
  windows and persisted, on top of the platform's text size.
- Reflow Paragraphs is off by default, global and persisted. Show Markdown is
  per window and not persisted.

### Editing

Every editor produces the same source for the same keys. The choices:

- Shift+Enter writes a backslash hard break (`\`), not two spaces. In a
  heading it acts as Enter.
- Enter at the start of a paragraph opens an empty paragraph above; in a
  quote it starts a new paragraph inside the quote; at the end of an opening
  fence it closes the block.
- Backspace at the start of a list item makes it a paragraph of the item
  above (a nested item moves out a level instead; a checkbox goes before the
  bullet). After a rule it deletes the rule; after a code block it moves into
  it.
- A block command on a line that already has that type turns it into a
  paragraph. Bold and friends act on the word at the cursor when nothing is
  selected.
- An item nested under a numbered item starts a sublist at 1.
- No automatic character substitution (smart quotes, dashes, autocorrect).
- Links to `.md`/`.markdown` files open in Margin; other files in their
  default app.
- Find ignores case by default and treats line breaks inside paragraphs as
  spaces. Replace edits in place when the match is plain text, otherwise
  deletes and retypes through the editing rules.
- Replacing a selection (typing or pasting over it, a spelling correction)
  edits in place when it lies in plain text, keeping the formatting around
  it; otherwise it deletes and then types through the editing rules.
- Print renders the document as shown, without comments, with the body at
  11pt in the light appearance.

### Comments

- A thread is a comment plus replies, each with a time and an author: the
  writer, for messages from the editor, or an agent, for messages from the
  CLI. Agents aren't told apart. Threads are numbered per document and
  numbers are never reused.
- Each message has a header row, as in Pages, Figma and GitHub: at its
  start the author, "You" or "Agent", and the time on one line ("Agent ·
  14:10"); at its end the message's buttons, Resolve (or Reopen) on the
  comment's row, and a menu for the rarer actions. Editing a message
  doesn't change its author.
- Anchors: edits elsewhere move them; replacing the anchored text re-anchors
  to the replacement (even inside formatting); insertions inside grow the
  anchor, insertions at its edges don't join it; deleting all of it detaches
  the thread, which stays where the text was and keeps the old quote.
- A new comment's span is trimmed of whitespace and hidden syntax; without a
  selection it takes the word at the cursor. The document is saved first.
- Cards never overlap: the focused card sits beside its text and the rest
  stack around it. When threads overlap, the cursor focuses the innermost.
- Clicks in the gutter never move the text cursor, and document commands do
  nothing while focus is in a card.
- A message longer than seven lines is cut off at three, with Show More
  below it, as Google Docs and Pages do; Show Less cuts it off again.
  Each message is expanded on its own, and stays so until the window
  closes; focusing a thread doesn't expand it.
- Agent activity is announced ("1 new reply, 2 comments resolved"): new
  threads, replies, resolves, reopens and deletes. What was there when the
  document opened is not.
- Unless Margin is active with the document's window in front, activity on
  an open document also posts a system notification: titled with the file
  name, saying what happened to which thread (`Resolved "quote"`) and the
  message, if any. Clicking it brings the document forward, focused on the
  thread when it is about one. Coming back to the window removes the
  document's notifications. They have no buttons, and the system's
  notification settings are the only switch.
- The writer can edit and delete any message, in resolved threads too;
  agents can't. Deleting the comment deletes its thread. An edit can't leave
  a message empty: deleting is its own command.
- Nothing asks for confirmation: resolving, reopening, editing, deleting
  and Resolve All are undoable, and undo shows the thread it brings back.
  All but editing can also be undone from the notification that reports
  them.
- Copy Open Comments format, with paths relative to the git repository when
  there is one:

  ```text
  I left comments on `/home/me/proj/plan.md`. Please address them.

  #3 `plan.md:3:8-3:17` "bold words"
    - User: Italic instead?
    - (2 earlier replies: `margin thread plan.md 3`)
    - User: Keep them bold, but fewer words.
  #1 `plan.md:4:1` (the commented text, "old step", was deleted)
    - User: Why?
  ```

  Items go in document order under their thread numbers. Each gives the
  comment and the latest message, each after its author, `User` or `Agent`; the replies between, which the agent saw
  on earlier copies, are left to `margin thread`, so copying again after
  each round doesn't paste the whole conversation again.

### CLI

The CLI's help and the agent skill follow this section.

- Locations are `file:line:column`, columns in characters, against the file
  as it is now; every command re-anchors first. Paths print relative to the
  current directory when they are under it.
- Without files, `comments` looks at the documents with open threads under
  the current directory; `--all`, anywhere.
- `comments` and `thread` take `--json`.
- The editor and the CLI can change a document's comments at the same time
  without losing either's changes.

A thread in text, as listings print it, under a header per document
(`plan.md: 2 open threads, 1 resolved (--resolved to show)`):

```text
#3 plan.md:12:10 (open)
  on "blue/green deploy"
  user · 14:02
    Why not canary?
  agent · 14:10
    Switched to canary in §2.
```

The status is `open`, `open, detached: the commented text was deleted`, or
`resolved`. Quotes print on one line, cut at 160 characters. In JSON:

```json
{
  "doc": "/abs/path/plan.md",
  "id": 3,
  "status": "open",
  "detached": false,
  "start": { "line": 12, "column": 10 },
  "end": { "line": 12, "column": 27 },
  "quote": "blue/green deploy",
  "messages": [
    { "author": "user", "at": "2026-09-29T12:02:00Z", "body": "Why not canary?" },
    { "author": "agent", "at": "2026-09-29T12:10:00Z", "body": "Switched to canary in §2." }
  ]
}
```

A message's author is `user` or `agent`: the CLI speaks to agents, so it
calls the writer the user. A resolved thread also has `resolved_at`.

The commands:

- `margin FILE…` (or `margin open FILE…`) opens documents in the editor and
  returns at once, after reporting files it can't open. `--foreground` stays
  attached until the editor quits.
- `margin comments [FILE…]` lists open threads; `--resolved` adds resolved
  ones.
- `margin thread FILE ID` shows one thread, open or resolved, in full. The
  JSON is the one thread's object.
- `margin reply FILE ID MESSAGE [--resolve]`, `margin resolve FILE ID
  [MESSAGE]`, `margin reopen FILE ID`, `margin delete FILE ID`.
- `margin add FILE --quote TEXT MESSAGE` starts a thread on TEXT, which must
  occur exactly once in the file; otherwise it is refused, listing the lines
  it occurs on, so the agent quotes more. Agents point at text by quoting it,
  as their edit tools do, rather than by line and column, which they count
  badly.

### Platforms

Every editor shares the core: Markdown analysis, editing rules, find, the
comment store and anchoring, and the CLI. Each platform provides the UI and
system integration, and routes every edit through the core.

- Each editor uses its platform's own pieces wherever one exists: menus,
  window chrome, dialogs, file pickers, the print dialog, system colors and
  fonts. It draws custom UI only for what the platform has no equivalent of
  (the comment gutter, the drawn Markdown blocks).
- Where platform conventions differ, each editor follows its own, even when
  that makes the editors differ; the behavior in this spec stays the same.
- A command's keys are, in order: the platform's own binding for it, if
  the platform has one; else Google Docs' binding on that platform; else
  Margin's own. A key the platform gives to something else (a system
  shortcut, a standard command, a character it types) stays with that, and
  the command takes a nearby combination. Margin's own commands keep the
  same letter on every platform, under that platform's modifiers.
- List shortcuts (numbered, bulleted, checklist) go by the number row's
  physical keys, as in Google Docs, so they work on any keyboard layout.
