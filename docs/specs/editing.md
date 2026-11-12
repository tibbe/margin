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
  heading it acts as Enter. Tables gives the rule for a table that shows
  as a grid.
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

## Tables

In a table that shows as a grid, a cell is a paragraph for cursor
movement, selection and find. For example, a triple-click selects the
cell.

- **Up and Down.** On the first line of a cell, Up goes to the last line of
  the cell above. On the last line of a cell, Down goes to the first line
  of the cell below. The cursor keeps its horizontal position. From the
  first or last row, Up and Down go out of the table.
- **Clicks.** A click or a drag anywhere in the box of a cell goes to the
  nearest position in that cell.
- **Missing cells.** A missing cell has no position. When an action puts
  the cursor in a missing cell, the cursor goes to the end of the last
  cell of that row.
- **Selection.** A selection is one range of the file. When the writer
  drags across cells, or uses Shift with an arrow key, the selection
  includes all text between its two ends, in source order.
- **Enter.** Enter goes to the end of the text in the cell below. In the
  last row, Enter first adds an empty row.
- **Shift+Enter.** Shift+Enter writes `<br>`.
- **Paste.** When the writer pastes text into a cell, each line break in
  the text becomes `<br>`.
- **Tab.** Tab selects the text of the next cell, in source order. In the
  last cell, Tab first adds an empty row.
- **Shift+Tab.** Shift+Tab selects the text of the previous cell, in source
  order. In the first cell, Shift+Tab does nothing.
- **Added rows.** An added row has an empty cell for each column. It uses
  the outer pipes of the row above, with one space on each side of each
  cell. For two columns, the row is `|  |  |`.
- **Cell edges.** Backspace at the start of a cell does nothing, and Delete
  at the end of a cell does nothing. This also applies to a table in a
  quote or a list item.
- **Deletion across cells.** A deletion across cells is a deletion of a
  selection that includes parts of two or more cells, or a part of a table
  and text outside it. Delete, Backspace, Cut, and text that the writer
  types or pastes over the selection all make a deletion.
- **Whole rows.** A deletion across cells removes each body row that it
  covers fully. If it covers all rows, it removes the table.
- **Header row.** If a deletion across cells covers the header row fully
  but not all rows, it removes the text of the header cells and keeps the
  row.
- **Partial rows.** From a row that it covers partly, a deletion across
  cells removes only text.
- **Table edge.** A deletion never joins a table to the text before or
  after it.
- **Backspace after a table.** On a line with text after a table,
  Backspace at the start of the line selects the table.
- **Delete before a table.** On a line with text before a table, Delete at
  the end of the line selects the table.
- **Empty line after a table.** On an empty line after a table, Backspace
  removes the line and puts the cursor at the end of the last cell.
- **Empty line before a table.** On an empty line before a table, Delete
  removes the line and puts the cursor at the start of the first cell.
- **Empty line next to text.** When a paragraph is on the other side of
  the empty line, the line stays: Backspace and Delete only move the
  cursor into the table. Without the line, the paragraph would join the
  table.
- **Cursor after a deletion.** After a deletion across cells, the cursor is
  where the selection started, and typed or pasted text goes there. If the
  deletion removed that row, the cursor goes to the start of the line
  after the removed rows. If no line follows them, the cursor goes to the
  end of the line before them.
- **Block commands.** Block commands do not change the rows of a table.
  This applies to Normal Text, the headings, the list commands, Quote,
  Code Block, Toggle Task, Indent and Outdent.
- **Block commands near a table.** On a selection that goes into a table,
  block commands change only the lines outside the table.
- **Block command menus.** Margin disables the menu items of block
  commands when the selection is fully in a table.
- **Find.** A find match never goes across two cells.

## Out of scope

- A paste of tab-separated data, for example cells from a spreadsheet,
  into many cells of a table.
