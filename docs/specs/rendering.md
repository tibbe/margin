# Rendering

## User Stories

1. As a writer, I want HTML, front matter and link reference definitions shown as dimmed source, and tables as grids that edit as text, so that nothing in the file is hidden or misrendered.
2. As a writer, I want images shown as images, local and remote, so that I see the screenshot or mockup a spec is about.
3. As a writer, I want Mermaid diagrams shown as diagrams, so that I read a flowchart as a flowchart, not as its source.
4. As a writer, I want an image or diagram that can't be shown to say why, so that I can fix it.
5. As a writer, I want the syntax of the blank line I'm on, and the fences of the code block I'm in, shown, so that I can edit what is otherwise invisible.
6. As a writer, I want a marker without its space (`#`, `-`) left as typed, so that a line isn't formatted before I mean it.
7. As a writer, I want to switch a window to raw source, keeping my place, so that I can see exactly what is in the file.
8. As a writer, I want hard-wrapped paragraphs to reflow to the window when I choose, without changing the file, so that I can read them comfortably.

## Decisions

- CommonMark with GitHub tables, strikethrough and task lists, and YAML front
  matter.
- HTML, front matter and link reference definitions show as dimmed
  monospace source. A `<br>` in a table cell is an exception (see Tables).
  Bare URLs and autolinks show as links, with the URL visible.
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

## Tables

A table shows as a grid. The grid fits the width of its block: the text
column, less the indent of a list or a quote. The text in each cell wraps
in its column, as in the automatic table layout of a browser.

- **Natural widths.** If each column fits at the width of its widest cell,
  each column has that width.
- **Shared widths.** If the columns do not fit at those widths, the grid
  fills the width of its block. Each column has at least the width of its
  longest word. The columns divide the remaining width. The part of each
  column is in proportion to the width that its text needs in addition to
  fit on one line.
- **Long words.** Sometimes the longest words of all columns do not fit
  together. Then the width of each column is in proportion to the width of
  its text on one line. A word that is too long for its column breaks
  between two characters.
- **Vertical position.** The text of a cell starts at the top of the cell.
  A row is as tall as its tallest cell.
- **Alignment.** The text of a cell has the alignment that the delimiter
  row gives to its column. The text of a column without an alignment
  starts at the start of the line.
- **Short and long rows.** A row can have fewer or more cells than the
  header. The grid has a column for each cell of its longest row. Where a
  row has no cell for a column, the grid shows an empty cell.
- **Line breaks.** A `<br>` tag in a cell shows as a line break. Margin
  hides its source.
- **Images.** An image alone in a cell shows as the image, as an image
  alone in its paragraph does. The image is never wider than its column.
  Its width counts in the width of its column on one line, but not as the
  longest word of its column.
- **Highlights.** The selection, comment highlights and find matches show
  in a cell as in a paragraph with the width of the column. For example, a
  selected line that continues on the next line of the cell goes to the
  right edge of the cell.
