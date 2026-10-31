# Editing

## User Stories

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

## Decisions

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
  column; Shift extends a selection over it whole. A click selects it too,
  and so do Backspace and Delete toward it, rather than joining its line to
  the text beside it. Selected, Delete removes it, typing replaces it, and
  copying gives its Markdown. Its source is edited in Show Markdown.
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
