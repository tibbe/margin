
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
answer through a CLI. Each choice below puts it at one point in the design
space, against the alternative it names. Other tools make some of these
choices; none makes them all.

- **The writer's own agent, in their project.** Margin has no AI of its
  own: the writer reviews with the coding agent they already run, which
  reads the codebase, runs commands and builds from the document. An
  assistant built into a document editor sees only the document. Agents
  take part through the `margin` CLI, like `tuicr` and `hunk`, and their
  edits and replies appear in the open editor as they happen.
- **Threads, not a chat.** A review raises many points at once. Each is a
  thread on its own text and is answered there. In a chat, the points and
  their answers share one stream, away from the text they are about.
- **Many copies, not one shared document.** Like code in git, a document can
  have many copies at once, each with its own review and its own agent, so
  several sessions work on one spec in parallel without mixing. Google Docs
  has one document that everyone edits.
- **The file is the source of truth.** The editor edits the Markdown text
  directly and never re-serializes it; an edit changes only the bytes it
  must, so diffs and merges in git keep working. Rich editors rewrite the
  file on save; Google Docs keeps no file in the project at all.
- **Markdown, because both sides write it.** Agents write Markdown well and
  people write it easily. In Margin it is an input method: typed syntax
  formats as you type and is hidden.
- **Rich, not plain.** A spec says some things best as a table, an image or
  a diagram (a Mermaid flowchart, say), so the editor shows them as such,
  not as their source.
- **Proportional, because review is reading.** Most of the writer's time
  goes to reading what the agent wrote, so the screen reads like a
  document, in proportional type, not like source in a monospace editor.
- **Comments beside the text, not in it.** Cards sit in the margin next to
  their text, as in Google Docs, so they never break up the reading the way
  comments between the lines of a code review do.
- **Changes marked in the document, not in a diff.** After an agent's
  edits the writer needs to know what to read again, so lines changed since
  the last commit have a bar beside them where the writer reads and
  comments. A diff view shows the changes away from the document and its
  comments.

User stories name two actors: the **writer** (the one person who
comments) and a **coding agent**.

## Files

### User Stories

1. As a writer, I want saving to change only the bytes I edited, so that diffs show only real changes.
2. As a writer, I want documents to save themselves, so that I never think about saving.
3. As a writer, I want to be asked before closing only when text would be lost, so that closing is quick but never destructive.
4. As a writer, I want untitled documents kept until I name them, and brought back after a crash, so that drafts are never lost.
5. As a writer, I want comments to move with a document I save under a new name, so that renaming doesn't lose my review.
6. As a writer, I want an agent's edits to an open document picked up, and merged with mine when they don't overlap, so that we can both work at once.
7. As a writer, I want to choose which version to keep when our edits overlap, so that nothing is lost without my say.

### Decisions

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

## Rendering

### User Stories

1. As a writer, I want HTML, front matter and link reference definitions shown as dimmed source, and tables as grids that edit as text, so that nothing in the file is hidden or misrendered.
2. As a writer, I want images shown as images, local and remote, so that I see the screenshot or mockup a spec is about.
3. As a writer, I want Mermaid diagrams shown as diagrams, so that I read a flowchart as a flowchart, not as its source.
4. As a writer, I want an image or diagram that can't be shown to say why, so that I can fix it.
5. As a writer, I want the syntax of the blank line I'm on, and the fences of the code block I'm in, shown, so that I can edit what is otherwise invisible.
6. As a writer, I want a marker without its space (`#`, `-`) left as typed, so that a line isn't formatted before I mean it.
7. As a writer, I want to switch a window to raw source, keeping my place, so that I can see exactly what is in the file.
8. As a writer, I want hard-wrapped paragraphs to reflow to the window when I choose, without changing the file, so that I can read them comfortably.

### Decisions

- CommonMark with GitHub tables, strikethrough and task lists, and YAML front
  matter.
- Tables show as grids.
- HTML, front matter and link reference definitions show as dimmed
  monospace source; bare URLs and autolinks as links with the URL visible.
  Checked tasks are struck through and dimmed; Heading 6 is dimmed.
- Bullets change shape with nesting depth. A code block sits on a tinted
  background and shows its language; a quote has a bar down its left side.
- An image alone in its paragraph, in a list item or a quote too, shows as
  the image; an image in running text shows as italic, link-colored alt
  text. Relative paths are from the document's folder; remote images load
  too.
- An image shows at its own size: at the default zoom, one point of the
  image (its pixels at its resolution) to one point on screen. It scales
  with the zoom but not with the platform's text size. It is never wider
  than the text column, and never stretched to fill it. A placeholder, a
  box on the code background, holds its place until its size is known;
  when the size arrives, the text being read stays where it is.
- An image that is missing or can't be read shows as its alt text (its file
  name, without one) after a broken-image mark; hovering it gives the path
  or URL and the reason.
- A `mermaid` code block shows as its diagram, in the page's colors: nodes
  on the code background with borders around them, text in the text color,
  lines and arrows dimmed. One that can't be drawn shows as an error block
  on the code background: the message saying what is wrong, in the error
  color, then the source with the line at fault marked. An unknown diagram
  type, or one Margin can't draw, says which. A broken diagram never shows
  its last good picture. Code blocks in other diagram languages (PlantUML,
  Graphviz) stay code blocks.
- A selected image or diagram is tinted with the selection color.
- Reflow Paragraphs is off by default, global and persisted. Show Markdown is
  per window and not persisted.

## Editing

### User Stories

1. As a writer, I want Enter to start a new paragraph and Shift+Enter to break the line within it, so that I get the break I mean.
2. As a writer, I want Enter in a list, quote or code block to continue it, and Enter on an empty item to leave the list, so that I never type markers.
3. As a writer, I want numbered lists renumbered only when they are numbered in sequence, so that `1.` `1.` `1.` lists stay that way.
4. As a writer, I want Backspace at the start of a heading, quote or item to turn it back into a paragraph, so that undoing a block type is one key.
5. As a writer, I want deleting the last character of formatted text to remove its markers, so that no empty `****` is left behind.
6. As a writer, I want splitting or partly un-formatting a phrase to leave well-formed Markdown on both sides, so that I never see stray markers.
7. As a writer, I want typing at the end of bold text to continue the bold only when the cursor is inside it, and never to extend a link, so that formatting starts and stops where I expect.
8. As a writer, I want copying to give balanced Markdown and pasting to insert Markdown, so that formatting survives the clipboard.
9. As a writer, I want the characters I type written exactly as typed, so that the file holds what I typed.
10. As a writer, I want search to match the text as shown, so that "bold text" finds `**bold** text`.
11. As a writer, I want replacing to keep formatting around a match, so that I can rename things safely.
12. As a writer, I want an image or diagram to act as one piece, so that I never break its syntax by accident.

### Decisions

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
- An image or diagram shown as such is one object, and the cursor never
  rests in its source. Left or Right toward it selects it, and the next
  press moves past it, as in Obsidian; Up and Down pass it keeping the
  column; Shift extends a selection over it whole. A click selects it too.
  Selected, Delete removes it, typing replaces it, and copying gives its
  Markdown. Its source is edited in Show Markdown.
- No automatic character substitution (smart quotes, dashes, autocorrect).
- Links to `.md`/`.markdown` files open in Margin; other files in their
  default app.
- Find ignores case by default and treats line breaks inside paragraphs as
  spaces. Replace edits in place when the match is plain text, otherwise
  deletes and retypes through the editing rules.
- Find matches what is shown. An image drawn as an image matches nothing,
  neither its alt text nor its path; alt text that is shown (an image in
  running text, a missing image) matches. A diagram matches the labels it
  draws, highlighted where they are drawn. Replace skips matches in
  diagrams, and Replace All says how many it left.
- Replacing a selection (typing or pasting over it, a spelling correction)
  edits in place when it lies in plain text, keeping the formatting around
  it; otherwise it deletes and then types through the editing rules.
- Print renders the document as shown, images and diagrams included once
  loaded, without comments or change marks, with the body at 11pt in the
  light appearance.

## Comments

### User Stories

1. As a writer, I want to select text and comment on it, so that feedback is attached to exactly the passage it is about.
2. As a writer, I want each thread's card beside its text in the margin, so that I read comments alongside the document.
3. As a writer, I want the cursor entering commented text to focus its thread, so that moving through the text moves through the review.
4. As a writer, I want to edit or delete any comment or reply, so that I can fix what I wrote or clear what no longer helps.
5. As a writer, I want long comments and replies cut short until I ask for the rest, so that one long message doesn't push the other threads out of view.
6. As a writer, I want each message to say whether I or an agent wrote it, so that I can follow who said what in a thread.
7. As a writer, I want resolving, resolving all, editing and deleting to be undoable, so that a slip doesn't lose a thread.
8. As a writer, I want resolved threads hidden unless I ask for them, so that the margin shows what is still open.
9. As a writer, I want a thread whose text was deleted to stay, showing what it was about, so that feedback is never silently lost.
10. As a writer, I want a thread to follow its text when it is reworded, so that a comment on "blue/green" follows the change to "canary".
11. As a writer, I want to be told when an agent adds, answers, resolves, reopens or deletes threads, even while I am in another app, so that I notice its answers when it replies asynchronously.
12. As a writer, I want to copy the open comments as a list with locations, so that I can paste a review into an agent's chat.
13. As a writer, I want to send the open comments to the agent waiting on a document in one step, so that I don't paste every round of review into its chat.
14. As a writer, I want to see whether an agent is waiting on a document or working on what I sent, so that I know whether answers are coming.
15. As a writer, I want one send to cover every document the agent is waiting on, so that I can review several specs and send them as one round.
16. As a writer, I want the documents of a round in their own windows, so that I can read them side by side.
17. As a writer, I want a send to reach only the agents waiting on those very files, so that reviews of other copies of a project (another clone or worktree) never mix.

### Decisions

- A thread is a comment plus replies, each with a time and an author: the
  writer, for messages from the editor, or an agent, for messages from the
  CLI. Agents aren't told apart. Threads are numbered per document and
  numbers are never reused. The numbers are for the CLI; the editor doesn't
  show them.
- Each message has a header row, as in Pages, Figma and GitHub: at its
  start the author, "You" or "Agent", and the time on one line ("Agent ·
  14:10"); at its end the message's buttons, Resolve (or Reopen) on the
  comment's row, and a menu for the rarer actions. Editing a message
  doesn't change its author.
- Commented text is highlighted, more strongly for the focused thread.
- A card has the card background and a thin border, and a rule between its
  messages. The focused card stands out from the others.
- A detached thread's card shows the quote below the comment's header row:
  in quotation marks, dimmed, italic and struck through, with the tooltip
  "The commented text was deleted".
- A new comment or reply is typed on its card, with Cancel and Comment (or
  Reply) below it.
- Clicking anywhere on a card, its text included, focuses the thread.
- A thread's anchor follows its text through edits, as the table after
  this list sets out.
- A new comment's span is trimmed of whitespace and hidden syntax; without a
  selection it takes the word at the cursor. A comment on an image or
  diagram is on all of its source: the whole object is tinted, and its card
  sits beside the object's top. The document is saved first.
  If its text is deleted before it is posted, it doesn't post (there is
  nothing to comment on) and the draft stays, so its words aren't lost.
- Cards never overlap. With no thread focused, each card sits beside its
  text, or just below the card above it if that one reaches further down.
- The focused card (or the draft) always sits beside its text, as in
  Google Docs. The other cards move out of its way, off the page if need
  be, until the focus moves.
- The page grows to fit cards that reach below the text. A clicked card that
  moves out of view is scrolled back into it.
- When threads overlap, the cursor focuses the innermost.
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
  All but editing can also be undone from the announcement that reports
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
- For several documents, as a round sends them, the first line names each
  by its absolute path (`` I left comments on `/home/me/proj/plan.md` and
  `/home/me/proj/spec.md`. ``), and each document's items follow in turn,
  after a blank line. The absolute paths say which copy of a project the
  comments are on, since the items' paths read the same in every copy.
- Send to Agent gives the open comments, in the Copy Open Comments format,
  to every agent waiting on the document with `margin wait`. Margin doesn't
  start agents or look for them: it reaches only one that waits, which any
  agent that can run a command can do. Nothing is queued, so the command is
  unavailable, saying why, while no agent is waiting or no thread is open
  in the round.
- A send covers a **round**: the documents its agents wait on, which an
  agent names with `margin wait FILE…`, and, when agents wait on
  overlapping documents, theirs too. Send to Agent in any of the round's
  windows sends the open comments on all of them, documents without open
  threads left out. The agent defines the round, so there is nothing to
  name, save or close, and a round lasts as long as the wait.
- Each of a round's documents has its own window, so they sit side by
  side, or in the platform's window tabs when the writer prefers tabs.
- Documents are keyed by canonical path, so the same file in two copies of
  a project is two documents, with their own threads and waiting agents.
- Beside the comment count the window shows the agent: waiting (Send to
  Agent is available, and says how many other documents the round has),
  working (from a send that covered the document, from any window, until an
  agent waits on it again) or neither. Working gives up after 30 minutes without
  agent activity on the document.
- Send to Agent's tooltip says what it would send, or what is missing: an
  agent waiting, or an open comment. When the round has other documents,
  it says how many ("Send open comments on this and 2 other documents to
  the agent"). A send's announcement says what went: "Sent 2 open comments
  to the agent", or "Sent 5 open comments on 3 documents to the agent" for a
  round.

How an anchor follows each kind of edit, shown on a thread on `quick fox`
in `the quick fox jumps`. Brackets mark the anchor after the edit.

| Edit | Anchor | After |
| --- | --- | --- |
| Elsewhere | Moves with it | `then the [quick fox] jumps` |
| Inside it | Takes it in | `the [quick brown fox] jumps` |
| Replacing all of it | Takes the new text | `the [lazy dog] jumps` |
| Insertion at an edge | Leaves it out | `the very [quick fox] jumps` |
| Across an edge | Loses what it covers | `the [quick].` |
| Deleting all of it | Detaches | `the jumps` |

Formatting doesn't change these: replacing `quick fox` in
`the **quick fox** jumps` gives `the **[lazy dog]** jumps`. Text with
only whitespace left counts as deleted. A detached thread stays where its
text was and keeps the old quote.

An outside change to the file gives only the new text, not the edits that
made it. The editor and the CLI work out the edits the same way, so they
put anchors in the same places:

- They compare the lines, then the words of the lines that changed.
- Words are compared whole, never letter by letter, so an anchor never ends
  up on part of a word.
- The edits found needn't be the ones the writer made. Rewriting
  `[the round]; its tooltip` as `the spec and its tooltip` is found as
  `round;` replaced by `spec and`, across the anchor's end, which leaves
  `[the ]spec and its tooltip`.

## Changes

### User Stories

1. As a writer, I want the lines changed since the last commit marked beside the text, so that I know what to review after an agent edits a document.
2. As a writer, I want to step to the next and previous change, so that I can review a long document without hunting for the marks.

### Decisions

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

## Window

- Light and dark follow the system's appearance. The accent color marks
  focus.
- The title is the document's name and the subtitle its folder (`~/…`
  under the home folder), or "Not saved yet" for an untitled document.
- The toolbar holds the open-comment count ("3 open comments", or "2
  resolved" when none are open), with the agent's state beside it, Send to
  Agent and a comment button. Every other command is in the menus.
- The text column holds about 100 characters, with the comment gutter to
  its right. The gutter is the page's margin: one background behind the
  text and the cards. Spacing and sizes are proportional to the text
  size, except an image's, which follows only the zoom. Zoom is shared by
  all windows and persisted, on top of the platform's text size.
- The body is in a proportional font, code and source shown as source in a
  monospace one, and cards in the interface font.
- Announcements ("Updated from disk", "1 new reply") are a short message
  that goes away by itself, with an Undo button when what it reports can be
  undone.

Colors go by role:

| Role | For |
| --- | --- |
| Background | The page, behind the text and the cards |
| Text | Body text |
| Headings | Headings 1–5 |
| Dimmed text | Syntax and source, times, Heading 6 |
| Borders | Cards, rules, quote bars |
| Accent | Focus, checked boxes |
| Selection | Selected text, a selected image or diagram |
| Code background | Code blocks, inline code, table headers |
| Card background | Cards |
| Comment highlight | Commented text, stronger for the focused thread |
| Find matches | Find matches, stronger for the current one |
| Links | Links, image alt text |
| Change bars | Added, changed and deleted lines |
| Errors | Diagram errors |

## Commands

Every editor has Margin's commands under these names and keys, beside the
platform's standard commands (New, Open, Save, Print, Close, Quit, Undo,
the clipboard, Find and its commands, text size, Full Screen), which keep
the platform's names and keys.

- A command's keys are, in order: the platform's own binding for it, if
  the platform has one; else Google Docs' binding on that platform; else
  Margin's own. A key the platform gives to something else (a system
  shortcut, a standard command, a character it types) stays with that, and
  the command takes a nearby combination. Margin's own commands keep the
  same letter on every platform, under that platform's modifiers.
- List shortcuts (numbered, bulleted, checklist) go by the number row's
  physical keys, as in Google Docs, so they work on any keyboard layout.
- Keyboard Shortcuts lists every shortcut.

Mod is the platform's main modifier (Command on the Mac, Ctrl elsewhere)
and Alt its second (Option on the Mac):

| Command | Keys |
| --- | --- |
| Show Markdown, Reflow Paragraphs | Mod+/, Alt+Z |
| Bold, Italic, Strikethrough, Inline Code | Mod+B, Mod+I, Mod+Shift+X, Mod+E |
| Link | Mod+K |
| Normal Text, Heading 1–6 | Mod+Alt+0, Mod+Alt+1…6 |
| Numbered List, Bulleted List, Checklist | Mod+Shift+7, 8, 9 |
| Quote, Code Block | Mod+Alt+Q, Mod+Alt+C |
| Indent, Outdent | Tab or Mod+], Shift+Tab or Mod+[ |
| Toggle Task | Mod+Enter |
| Open Link | Mod+click under the pointer; Alt+Enter at the cursor |
| Comment on Selection | Mod+Alt+M |
| Copy Open Comments | Mod+Shift+C |
| Send to Agent | Mod+Shift+Enter |
| Next Comment, Previous Comment | Mod+Alt+Down, Mod+Alt+Up |
| Next Change, Previous Change | Mod+Alt+Shift+Down, Mod+Alt+Shift+Up |
| Reply, Post | Mod+Alt+R, Mod+Enter |
| Leave Comment, close Find | Escape |

## CLI

### User Stories

1. As a coding agent, I want the open threads with `file:line:column` locations against the file as it is now, so that I can find what was asked even after my own edits.
2. As a coding agent, I want to find documents with open threads under the current directory, but not in other worktrees nested in it, so that I don't need to be told the file and don't pick up another checkout's review.
3. As a coding agent, I want a thread's quote as the file's exact Markdown source, so that I can find and edit the text.
4. As a coding agent, I want to reply, resolve and reopen, so that I report what I did where the writer will see it.
5. As a coding agent, I want to start threads on quoted text, with ambiguous quotes refused, so that my question lands on the right text.
6. As a coding agent, I want to open a document for review and return at once, so that my shell isn't blocked.
7. As a coding agent, I want to wait until the writer sends the comments on the documents I name, all of them at once, so that I pick up each round of review without being told in chat.
8. As a coding agent, I want JSON output, so that I can process threads reliably.
9. As a coding agent, I want the same commands and output on every platform, so that one skill works everywhere.

### Decisions

The CLI's help and the agent skill follow this section.

- Locations are `file:line:column`, columns in characters, against the file
  as it is now; every command re-anchors first. Paths print relative to the
  current directory when they are under it.
- Without files, `comments` looks at the documents with open threads under
  the current directory, skipping directories that hold their own `.git`
  (other worktrees and repositories nested in it); `--all`, anywhere.
- `comments`, `thread` and `wait` take `--json`.
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
  returns as soon as the editor shows them, after reporting files it can't
  open. It fails if the editor hasn't shown them within 60 seconds, the
  default wait for an Apple event reply. `--foreground` stays attached until
  the editor quits.
- `margin comments [FILE…]` lists open threads; `--resolved` adds resolved
  ones.
- `margin thread FILE ID` shows one thread, open or resolved, in full. The
  JSON is the one thread's object.
- `margin wait FILE…` waits until the writer sends a round that covers
  its documents, prints the open comments on all of them as a round sends
  them, followed by the command to wait for the next round, and exits.
  `--json` prints the documents' open threads as `comments --json` does. An agent that runs
  commands in the background works on while it waits, and hears when the
  command exits.
- `margin wait` also exits when no editor window shows any of its
  documents, whether none did when it started or the writer closed them
  or quit, since no comments can come then. It says so and what to do
  next: open the documents first, or stop waiting. With `--json` it prints
  `[]`, and the explanation goes to stderr.
- `margin reply FILE ID MESSAGE [--resolve]`, `margin resolve FILE ID
  [MESSAGE]`, `margin reopen FILE ID`, `margin delete FILE ID`.
- `margin add FILE --quote TEXT MESSAGE` starts a thread on TEXT, which must
  occur exactly once in the file; otherwise it is refused, listing the lines
  it occurs on, so the agent quotes more. Agents point at text by quoting it,
  as their edit tools do, rather than by line and column, which they count
  badly.

## Platforms

- Each editor uses its platform's own pieces wherever one exists: menus,
  window chrome, dialogs, file pickers, the print dialog, system colors and
  fonts. It draws custom UI only for what the platform has no equivalent of
  (the comment gutter, the drawn Markdown blocks).
- Where platform conventions differ, each editor follows its own, even when
  that makes the editors differ; the behavior in this spec stays the same.
