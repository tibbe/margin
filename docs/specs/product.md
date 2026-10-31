
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
- **The file is the source of truth.** Saving adds no noise to the file's
  diffs: they show only what was edited.
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

## Areas

| Area | What it covers |
| --- | --- |
| [Files](files.md) | Opening, saving, drafts and changes made outside Margin. |
| [Rendering](rendering.md) | How the Markdown in a document is shown. |
| [Editing](editing.md) | What keys, commands, the clipboard and Find do to the source. |
| [Comments](comments.md) | Threads in the margin, and sending them to a waiting agent. |
| [Changes](changes.md) | Marking the lines changed since the last commit. |
| [CLI](cli.md) | The `margin` commands agents use to read and answer threads. |

The UX that holds across areas, the window, the commands and their keys, is in
[the design system](design_system.md).
