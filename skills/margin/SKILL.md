---
name: margin
description: Read and answer the user's comments on Markdown documents with the `margin` CLI, and open documents in the Margin editor for review. Use when the user left comments on a doc, asks to address or resolve them, wants a doc opened for review or asks you to wait for their comments, or refers to what they have open or selected in Margin.
---

# Margin

Margin is the user's Markdown editor. They leave comments on a document the
way they would in Google Docs: each **thread** is anchored to a span of the
document's text. The `margin` CLI is how you read threads and reply to
them. `margin --help` lists every command and flag.

Threads are numbered per document (`#3`). The number is for the CLI: the
editor doesn't show it, so when you write to the user about a thread, name
it by the text it is on. Locations are `file:line:column`, 1-based,
computed against the file as it is now.

## Address comments

1. **Find the document.** Use the file the user named. Otherwise run
   `margin comments` (documents under the current directory with open threads).
2. **Read the threads:** `margin comments FILE` prints each open thread with
   its location and the text it is on; read the document around them.
   `--json` gives the same threads as data.
3. **Work every open thread.** Change the document with your normal editing
   tools, or answer in the thread, whichever the comment asks for. Then reply
   on the thread saying what you did:

   ```bash
   margin reply FILE 3 "Switched the rollout to canary in §2."
   ```

   Leave threads open: the user resolves them once they are satisfied.
   Resolve (`margin resolve FILE ID`) only when the user asks you to.
4. **Check before reporting back:** rerun `margin comments FILE`. The step is
   done when every open thread ends with an `agent` message. The user may keep
   commenting while you work, so this can surface new threads and follow-ups.

Edits to the file show up in the open editor at once, and threads stay
attached to their text as it moves. Read positions from a fresh
`margin comments` rather than reusing line numbers from before your edits.

## Open a document for review

When you have written a document the user should review (a plan, a spec):

1. `margin open FILE`. It returns as soon as the document appears in the
   editor.
2. Wait for the review, below, and tell the user the document is open and
   you are waiting for them to send their comments.

## Wait for the review

`margin wait FILE` returns when the user clicks Send to Agent in the editor,
and prints that round's open comments with their locations. It also
returns when the document isn't open in the editor, since then no comments
can come. Run it in the background when your shell tool can, so you are
woken when it exits and stay free meanwhile; otherwise run it in the
foreground. Each time it exits, do what it prints: address the comments it
printed, as above, and run `margin wait FILE` again for the next round; or
stop waiting when it says the document isn't open.

The review is over when the user says so, or when `margin wait` says the
document isn't open. Until then, a round is done when every thread it
printed ends with your reply and `margin wait` is running again: that is
what tells the editor you are ready.

## Reference

- **The quote is Markdown source.** A thread's quote, and the `--quote` text
  of `margin add`, is the file's text including `**`, backticks and link
  syntax, exactly as it appears in the file.
- **One thread in full:** `margin thread FILE ID` prints a thread with all
  its replies, open or resolved. Comments the user pastes from Margin give
  each thread's comment and latest message only, with a line like
  `(2 earlier replies: margin thread plan.md 3)`. Run that command when
  you no longer have those replies in context.
- **Detached threads:** the text a thread was anchored to was deleted. The
  thread still needs an answer; its quote shows what it was about.
- **Your own threads.** `margin add FILE --quote "text" "question"` starts a
  thread, for when the user asks you to review a document or you need to ask
  about specific text. The quote must occur once in the file; if it occurs
  more often, the error lists its lines, and you quote more of the text
  around it.
- **Authors.** Each message is marked `user` or `agent` (`author` in the
  JSON; `User:` and `Agent:` in comments the user pastes). Every message
  sent through the CLI is `agent`, yours or another agent's; the rest are
  the user's, from the editor. A thread's first message is its comment,
  and a `user` message after an `agent` one is a follow-up to answer.
