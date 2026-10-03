# Margin on macOS: design system

Margin on macOS follows Apple's Human Interface Guidelines and the
conventions of Apple's document apps (TextEdit, Pages, Notes).

## Foundations

### Color

The spec's color roles are system semantic colors, so every appearance,
accent and accessibility setting (Increase Contrast, Reduce Transparency)
works, but for one color of Margin's own:

- `commentHighlightColor`: Apple's purple author color, so commented text
  never looks like a find match (yellow). In light mode Pages' comment
  fills, `#EDD1FE` (`#DCAFFD` for the focused thread); in dark mode, where
  Pages has none, Notes' first participant color `#A477EC` at 20% (40%
  focused).

| Role | Color |
| --- | --- |
| Background | `textBackgroundColor` |
| Text | `textColor` |
| Headings | `labelColor` |
| Dimmed text | `secondaryLabelColor` |
| Borders | `separatorColor` |
| Accent | `controlAccentColor` |
| Selection | `selectedTextBackgroundColor` |
| Code background | `quaternarySystemFill` |
| Card background | `controlBackgroundColor` |
| Comment highlight | `commentHighlightColor` |
| Find matches | `findHighlightColor` at 35% (90% for the current match) |
| Links | `linkColor` |
| Change bars: added, changed, deleted | `systemGreen`, `systemBlue`, `systemRed` |
| Errors | `systemRed` |

### Type

- Body: the system font (SF Pro) at 15pt, before zoom.
- Code: SF Mono.
- Cards: the system font at the regular and small sizes.
- A Settings window picks the body font (SF Pro or New York) and size.

### Geometry

- The page is centered: the text alone while there are no cards, the text
  and the cards together once there are. The first card moves the text
  left, and the last one resolved moves it back. Cards are 300pt wide,
  growing to 400pt with half of any spare width.
- Change bars are 3pt wide with round ends, 15pt left of the text; a
  deletion's triangle is 5pt wide and 8pt tall.

### Feedback

Announcements are a small banner at the bottom of the window, on the HUD
material, also read out by VoiceOver, that fades after 3 seconds, or 6 when it has an Undo button.
Apple sets no duration, and its HIG
[discourages](https://developer.apple.com/design/human-interface-guidelines/accessibility)
views that dismiss on a timer, so a banner only repeats what the window
shows anyway: its Undo is the Edit menu's, and new replies are on their
cards.

## Files

Documents follow the spec's saving rules rather than the Mac's own autosave,
so there is no version history (Revert To ▸ Browse All Versions) and no
"changed by another application" alert.

- Standard window with a unified toolbar, and the document proxy icon
  beside the title. Documents save themselves, so the edited dot on the
  close button only shows while a save is failing, or on an untitled
  document with text.
- Saves keep the file's Finder tags and extended attributes too.
- **Save As…** writes the document under a new name and continues there;
  its comments move along. **Rename…** and **Move To…** move the file itself,
  with its comments. **Duplicate** opens an untitled copy, comments included.
  **Revert to Last Opened** is one undoable step.
- A change an agent makes to the file, or a merge, is one undo step ("Undo
  Outside Change"), so earlier steps stay undoable.
- Comment changes are on the Edit menu's Undo too, in order with text
  edits.
- Windows reopen where they were after a relaunch, and documents can share a
  window as tabs; "+" in the tab bar opens an untitled document.
- The conflict ("Keep My Version", "Load Disk Version") and Unsaved Changes
  prompts are sheets. Unsaved Changes uses the Mac wording: "Do you want to
  save the changes made to “name”?" with Save…, Cancel and Don’t Save.

## Rendering

- Code blocks' boxes have a 6pt radius; quote bars are rounded.
- Tables are grids: the `|`s, the cells' padding and the delimiter row are
  hidden, columns take their widest cell's width and the delimiter row's
  alignment, separator-colored lines run between the rows and the columns,
  and the header row is bold and tinted like a code block. A row stays on
  one line.
- The grid is in a box with a 6pt radius. Cells have 10pt of padding at the
  sides and 6pt above and below. Text is at 94% of body size. A table wider
  than the page is cut off at its right edge.
- An image path starting with `/` is from the file system's root.
- Image placeholders and diagram error blocks have a 6pt radius, as code
  blocks' boxes do.
- The broken-image mark is the `photo.badge.exclamationmark` symbol, and a
  diagram error's header starts with `exclamationmark.triangle.fill`.

## Editing

- Clicking the left margin places the cursor.
- **No substitutions:** smart quotes, smart dashes, text replacement,
  autocorrect, link detection and inline predictions are off.
- **Spelling** underlines are on, except in code, links and hidden syntax;
  the right-click menu offers corrections, Ignore Spelling and Learn
  Spelling. Grammar is off.
- **Input methods** (Japanese, Chinese, dead keys) compose where typed text
  would go (after a link, not inside it). Text being composed keeps its
  marked-text underline and is styled once committed. Press-and-hold accents
  replace the letter like typing does.
- **Edits the system makes** (deleting a word, Transpose, spelling
  corrections) follow the same editing rules as typing.
- Text can't be dragged, and Writing Tools aren't available in the
  document.

## Comments

- Clicking empty gutter space leaves the focused thread and returns the
  keyboard to the text, with the cursor where it was. Over the gutter the
  pointer is the arrow.
- Dragging across a card's text, or double-clicking it, selects it, so the
  pointer over the text is the I-beam.
- Comment cards have an 8pt corner radius and a 1pt border. Cards float
  beside the text, so their corners are rounder than those of the blocks in
  it (6pt). The focused card (or the draft) is raised on a soft shadow, as
  the open comments of Pages, Ulysses and Final Draft are; no accent
  outline. Resolved cards are at 70% opacity.
- A comment or reply has no field border or focus ring. Below a hairline,
  bezel-less text buttons: Cancel in the secondary label color, Comment or
  Reply in the accent color, greyed while empty.
- A message's header row is 28pt tall. The author is in the small system
  font, semibold, in the label color, "Agent" after a `sparkles` symbol (as
  Copilot marks its reviews); the time is in the secondary label color.
  Resolve is `checkmark.circle` (`arrow.uturn.backward.circle` to reopen),
  and the menu a "…" button (`ellipsis.circle`) with Edit and Delete. The
  symbols are at the regular system font's size, each in a 28×28pt target
  (the HIG's default control size). The symbols, not the targets, line up
  with the text's edge; the targets reach into the card's padding. All the card's buttons show while the
  pointer is over the card or the card is focused; hidden, they keep their
  room and stay reachable with VoiceOver.
- A message longer than seven lines ends in "…" at its third line, with a
  bezel-less Show More text button in the accent color below it, at the
  small system font's size, as in Pages' comments; expanded, the button
  reads Show Less. It shows whether or not the pointer is over the card,
  since it tells that text is hidden. Clicking it focuses the thread, as
  any click on the card does.
- Edit turns the message into a box on the card holding its text, with
  Cancel and Save. Undo is named for the step: Undo Edit, Undo Delete
  Reply, Undo Delete Comment.
- In the toolbar, Send to Agent is `paperplane`. Everything else is in the
  menu bar.
- While the agent works on a send, the count reads "2 open comments ·
  Agent working".
- The toolbar holds only standard items and plain text. On the macOS 26
  toolbar, a custom view such as a spinner is drawn inside the buttons'
  glass or in a pill of its own, and a badge is always the system's
  notification red, which reads as needing attention. So the agent's state
  is words in the count, not a spinner or a badge.

System notifications (see the spec for when) are one per thread change,
as Mail and Messages post one per message, grouped by document
(`threadIdentifier`) so they stack with "+N more". The subtitle is
`"quote"`, `Resolved "quote"`, `Reopened "quote"`, `New comment on "quote"`
or `Deleted "quote"`, and the body the message. With previews hidden they
say "New reply", "Comment resolved" and the like. They play the default
sound; the Dock icon has no badge. Margin asks for permission the first time
agent activity is announced while it is frontmost, as Apple advises asking
in context, and posts nothing before then.

Open questions:

- Is a toolbar worth having at all, or should the window be title-only like
  Notes' full-screen editor?

## Commands

### Menu bar

The standard menus, with Margin's commands where Mac users look for them:

- **Margin**: About Margin, Services, Hide, Hide Others, Show All, Quit.
- **File**: New, Open…, Open Recent, Close, Save, Save As…, Duplicate,
  Rename…, Move To…, Revert to Last Opened, Page Setup…, Print….
- **Edit**: Undo, Redo, Cut, Copy, Paste, Select All; Find ▸ Find…, Find and
  Replace…, Find Next, Find Previous, Use Selection for Find; Spelling;
  Emoji & Symbols (added by the system).
- **Format**: Normal Text, Heading 1–6; Bulleted List, Numbered List,
  Checklist, Quote, Code Block; Bold, Italic, Strikethrough, Inline Code,
  Link…; Indent, Outdent, Toggle Task, Open Link.
- **Comments**: Comment on Selection, Reply, Next Comment, Previous Comment;
  Resolve (Reopen), Edit, Delete, for the focused thread (Edit edits its
  comment); Copy Open Comments, Send to Agent, Resolve All, Show Resolved.
- **View**: Show Markdown, Reflow Paragraphs; Next Change, Previous Change;
  Larger Text, Smaller Text, Actual Size; Enter Full Screen.
- **Window**: Minimize, Zoom, Bring All to Front.
- **Help**: Keyboard Shortcuts, a window listing every shortcut, read from
  the menus.

The text's right-click menu starts with Comment on Selection, above the
system's own items, as in Pages and Preview. A card's right-click menu has
Reply and Resolve (Reopen) for the thread, then Edit and Delete for the
message clicked.

### Key bindings

The standard commands (New, Open, Save, Print, Close, Quit, Undo, the
clipboard, Find and its commands, Bold, Italic, Enter Full Screen, Larger
and Smaller) have Apple's [standard keyboard
shortcuts](https://developer.apple.com/design/human-interface-guidelines/keyboards#Standard-keyboard-shortcuts).
Find and Replace is Cmd+Option+F, and Actual Size Cmd+0.
Margin's own commands take the spec's keys, with Command for Mod, Option
for Alt and Return for Enter, but for the keys macOS gives to something
else:

- **Cmd+E** is Use Selection for Find, so Inline Code is Cmd+Shift+E.
- **Option+letter types characters** (Option+Z is Ω), so no command is
  Option-only: Reflow Paragraphs is Cmd+Option+Z, and Open Link at the
  cursor Cmd+Option+Return.
- **Cmd+?** opens the Help menu's search, so Keyboard Shortcuts has no key;
  it is in the Help menu.

Standard shortcuts Margin gives to its own commands, since it has no use
for theirs:

- **Cmd+[ and Cmd+]** (left- and right-align) are Outdent and Indent.
- **Cmd+Option+C** (copy style) is Code Block.
- **Cmd+Shift+C** (the Colors window) is Copy Open Comments.
- **Cmd+Option+M** (minimize all windows) is Comment on Selection.

Open question: Cmd+Option+Q (Quote) sits next to Cmd+Q; is that too close?

## System

### Lifecycle

- One instance. `margin FILE…` creates files that don't exist yet before
  opening them, rather than on first save: Finder only opens existing
  files.
- Margin is an editor for Markdown files in Finder's Open With, and opens
  them on double-click.
- The app keeps running when its last window closes, as Mac document apps
  do; clicking its Dock icon then shows the Open panel.
- Quitting keeps untitled documents and asks, one window at a time, only
  about text whose save failed; Don't Save finishes quitting.
