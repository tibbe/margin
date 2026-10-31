# CLI

## User Stories

1. As a coding agent, I want the open threads with `file:line:column` locations against the file as it is now, so that I can find what was asked even after my own edits.
2. As a coding agent, I want to find documents with open threads under the current directory, but not in other worktrees nested in it, so that I don't need to be told the file and don't pick up another checkout's review.
3. As a coding agent, I want a thread's quote as the file's exact Markdown source, so that I can find and edit the text.
4. As a coding agent, I want to reply, resolve and reopen, so that I report what I did where the writer will see it.
5. As a coding agent, I want to start threads on quoted text, with ambiguous quotes refused, so that my question lands on the right text.
6. As a coding agent, I want to open a document for review and return at once, so that my shell isn't blocked.
7. As a coding agent, I want to wait until the writer sends the comments on the documents I name, all of them at once, so that I pick up each round of review without being told in chat.
8. As a coding agent, I want JSON output, so that I can process threads reliably.
9. As a coding agent, I want the same commands and output on every platform, so that one skill works everywhere.

## Decisions

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
calls the writer the user. A resolved thread also has `resolved_at`, and
`resolved_by`, `user` or `agent`.

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
  its documents, prints what the send gives on all of them, the open
  threads and those the writer resolved since the last send, followed by
  the command to wait for the next round, and exits. `--json` prints the
  same threads as `comments --json` does. An agent that runs
  commands in the background works on while it waits, and hears when the
  command exits.
- `margin wait` also exits when no editor window shows any of its
  documents, whether none did when it started or the writer closed them
  or quit, since no comments can come then. It says so and what to do
  next: open the documents first, or stop waiting. When the writer closed
  them, it first prints the threads the writer resolved since the last
  send. With `--json` it prints those threads, or `[]`, and the
  explanation goes to stderr.
- `margin reply FILE ID MESSAGE [--resolve]`, `margin resolve FILE ID
  [MESSAGE]`, `margin reopen FILE ID`, `margin delete FILE ID`.
- `margin add FILE --quote TEXT MESSAGE` starts a thread on TEXT, which must
  occur exactly once in the file; otherwise it is refused, listing the lines
  it occurs on, so the agent quotes more. Agents point at text by quoting it,
  as their edit tools do, rather than by line and column, which they count
  badly.
