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

## Open documents for review

Whenever you open documents in Margin for the user to comment on:

1. `margin open FILE…`. It returns as soon as the documents appear in the
   editor.
2. Start `margin wait` on the same documents at once, as in Wait for the
   review below, and tell the user the documents are open and you are
   waiting for them to send their comments.

Opening is done only when `margin wait` is running on every document you
opened.

## Wait for the review

`margin wait FILE…` returns when the user clicks Send to Agent in the
editor, and prints the round on every document you named: the open threads,
with their locations, and the threads the user resolved since the last
round. A resolved thread means the user took your answer: it is done. When
a review spans several documents, name them all in one `margin wait`: one
send from any of their windows covers them all. It also returns when none
of the documents is open in the editor, since then no comments can come,
after printing the threads the user resolved before closing them. Run it in
the background when your shell tool can, so you are woken when it exits and
stay free meanwhile; otherwise run it in the foreground. Each time it
exits, do what it prints: address the comments it printed, as above, and
run the `margin wait` command it prints again for the next round; or stop
waiting when it says the documents aren't open.

The review is over when the user says so, or when `margin wait` says the
documents aren't open. Until then, a round is done when every open thread
it printed ends with your reply and `margin wait` is running again: that is
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
- **Detached threads:** the text a thread was anchored to was deleted. A
  thread keeps to what is left of its text, so rewriting all of it, even
  one word, detaches it too. The thread still needs an answer; its quote
  shows what it was about.
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
