# Comments

## User Stories

1. As a writer, I want to select text and comment on it, so that feedback is attached to exactly the passage it is about.
2. As a writer, I want each thread's card beside its text in the margin, so that I read comments alongside the document.
3. As a writer, I want the cursor entering commented text to focus its thread, so that moving through the text moves through the review.
4. As a writer, I want to edit or delete any comment or reply, so that I can fix what I wrote or clear what no longer helps.
5. As a writer, I want long comments and replies cut short until I ask for the rest, so that one long message doesn't push the other threads out of view.
6. As a writer, I want each message to say whether I or an agent wrote it, so that I can follow who said what in a thread.
7. As a writer, I want resolving, resolving all, editing and deleting to be undoable, so that a slip doesn't lose a thread.
8. As a writer, I want resolved threads hidden unless I ask for them, so that the margin shows what is still open.
9. As a writer, I want a thread whose text was deleted to stay, showing what it was about, so that feedback is never silently lost.
10. As a writer, I want a thread to stay on the text it was about for as long as any of it is left, never moving onto text that replaced it, so that a comment is never shown against words it wasn't about.
11. As a writer, I want to be told when an agent adds, answers, resolves, reopens or deletes threads, even while I am in another app, so that I notice its answers when it replies asynchronously.
12. As a writer, I want to copy the open comments as a list with locations, so that I can paste a review into an agent's chat.
13. As a writer, I want to send the open comments to the agent waiting on a document in one step, so that I don't paste every round of review into its chat.
14. As a writer, I want to see whether an agent is waiting on a document or working on what I sent, so that I know whether answers are coming.
15. As a writer, I want one send to cover every document the agent is waiting on, so that I can review several specs and send them as one round.
16. As a writer, I want the documents of a round in their own windows, so that I can read them side by side.
17. As a writer, I want a send to reach only the agents waiting on those very files, so that reviews of other copies of a project (another clone or worktree) never mix.
18. As a writer, I want a send to tell the agent which threads I resolved, even when I leave nothing open, so that it knows I took its answers there.

## Decisions

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
- Send to Agent sends the document's changes to every agent waiting on it
  with `margin wait`: its open threads, and the threads the writer
  resolved since its last send, or, before any, since an agent first
  waited on it. Threads an agent resolved are left out. When the
  writer closes the round's documents, the waiting agents get the resolved
  threads alone.
- A send is in the Copy Open Comments format. A resolved thread's item
  ends with `- User resolved the thread.`, and the first line says what
  the send holds: `` I left comments on `…/plan.md` and resolved others.
  Please address the open ones. ``, or, with none open, `` I resolved
  comments on `…/plan.md`. ``
- Margin doesn't start agents or look for them: it reaches only one that
  waits, which any agent that can run a command can do. Nothing is queued,
  so the command is unavailable, saying why, while no agent is waiting or
  the round has no changes.
- A send covers a **round**: the documents its agents wait on, which an
  agent names with `margin wait FILE…`, and, when agents wait on
  overlapping documents, theirs too. Send to Agent in any of the round's
  windows sends what changed on all of them, documents where nothing did
  left out. The agent defines the round, so there is nothing to
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
- Send to Agent's tooltip is the command's name, or, when the round has
  other documents, says how many ("Send this and 2 other documents to the
  agent"). While the command is unavailable, it says what is missing: an
  agent waiting, or a change since the last send. A send's announcement says what went: "Sent 2 open comments to
  the agent", "Sent 2 open and 1 resolved comment to the agent", or "Sent 5
  open comments on 3 documents to the agent" for a round.

How an anchor follows each kind of edit, shown on a thread on `quick fox`
in `the quick fox jumps`. Brackets mark the anchor after the edit.

| Edit | Anchor | After |
| --- | --- | --- |
| Elsewhere | Moves with it | `then the [quick fox] jumps` |
| Insertion inside it | Takes it in | `the [quick brown fox] jumps` |
| Insertion at an edge | Leaves it out | `the very [quick fox] jumps` |
| Deleting part of it | Loses that part | `the [quick].` |
| Deleting all of it | Detaches | `the jumps` |

The anchor is on characters: its first and last, and those between. A
replacement is a deletion followed by an insertion, so replacing all of the
text detaches the thread (`the lazy dog jumps`), and replacing a word at an
edge leaves the new word out (`the slow[ fox] jumps`). Markup is text like
any other: making the text bold inserts `**` at its edges, giving
`the **[quick fox]** jumps`. Text with only whitespace left counts as
deleted. A detached thread stays where its text was, after any text that
replaced it, and keeps the old quote.

An outside change to the file gives only the new text, not the edits that
made it. The editor and the CLI work out the edits the same way, so they
put anchors in the same places:

- They compare the lines, then the words of the lines that changed.
- Words are compared whole, never letter by letter, so an anchor never ends
  up on part of a word, and a word rewritten even slightly (`colour` to
  `color`) counts as deleted.
- The edits found needn't be the ones the writer made. Rewriting
  `[the round]; its tooltip` as `the spec and its tooltip` is found as
  `round;` replaced by `spec and`, which deletes the anchor's last word and
  leaves `[the ]spec and its tooltip`.
