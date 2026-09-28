# Margin on macOS: design system

Margin on macOS follows Apple's Human Interface Guidelines and the
conventions of Apple's document apps (TextEdit, Pages, Notes).

## Look

- Standard window with a unified toolbar. The title is the document name,
  the subtitle its folder (`~/…`, or "Not saved yet"), with the document
  proxy icon. Documents save themselves, so the edited dot on the close
  button only shows while a save is failing, or on an untitled document with
  text.
- Light and dark follow the system appearance. The system accent color marks
  focus.
- The gutter is the page's margin: one text background behind the text and
  the cards, with no separator. Not a comments sidebar as in Pages: that
  would be a list, not notes beside their lines.
- Clicking the left margin places the cursor. Over the gutter the pointer is
  the arrow.
- Comment cards: the control background, an 8pt corner radius and a 1pt
  separator border; the focused card's border is the accent color at 2pt.
  Resolved cards are at 70% opacity.
- Code blocks sit in a tinted box with a 6pt radius; quote bars are rounded.
- Toolbar: the open-comment count and a comment button. Everything else is
  in the menu bar.

Open questions:

- Should cards use a material (vibrancy) or a plain background?
- Is a toolbar worth having at all, or should the window be title-only like
  Notes' full-screen editor?

## Color

System semantic colors, so every appearance, accent and accessibility
setting (Increase Contrast, Reduce Transparency) works.

| Role | Color |
| --- | --- |
| Background | `textBackgroundColor` |
| Text | `textColor` |
| Headings | `labelColor` |
| Dimmed text (syntax, times, H6) | `secondaryLabelColor` |
| Borders, rules, quote bars | `separatorColor` |
| Accent, focus, checked boxes | `controlAccentColor` |
| Code background | `quaternarySystemFill` |
| Card background | `controlBackgroundColor` |
| Comment highlight | `systemYellow` at 30% (60% focused); 22% and 45% in dark mode |
| Find matches | `findHighlightColor` at 35% (90% for the current match) |
| Links | `linkColor` |

## Type

macOS has no desktop-wide document font, so Margin needs its own choice.

- Body: the system font (SF Pro) at 15pt, before zoom.
- Code: SF Mono.
- Cards: the system font at the regular and small sizes.
- A Settings window picks the body font (SF Pro or New York) and size.

Open questions:

- Do the spec's heading sizes need adjusting for SF Pro or New York?
- Should Margin follow a system text size setting, if macOS offers one to
  third-party apps?

## Menu bar

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
  Copy Open Comments, Resolve All, Show Resolved.
- **View**: Show Markdown, Reflow Paragraphs; Larger Text, Smaller Text,
  Actual Size; Enter Full Screen.
- **Window**: Minimize, Zoom, Bring All to Front.
- **Help**: Keyboard Shortcuts, a window listing every shortcut, read from
  the menus.

The text's right-click menu starts with Comment on Selection, above the
system's own items, as in Pages and Preview.

## Documents

Documents follow the spec's saving rules rather than the Mac's own autosave,
so there is no version history (Revert To ▸ Browse All Versions) and no
"changed by another application" alert.

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

## Text input

- **No substitutions:** smart quotes, smart dashes, text replacement,
  autocorrect and link detection are off. Inline predictions are on:
  accepting one inserts text like typing does.
- **Spelling** underlines are on, except in code, links and hidden syntax.
  Grammar is off.
- **Input methods** (Japanese, Chinese, dead keys) compose where typed text
  would go (after a link, not inside it). Text being composed keeps its
  marked-text underline and is styled once committed. Press-and-hold accents
  replace the letter like typing does.
- **Edits the system makes** (deleting a word, Transpose, dragging text,
  spelling corrections, Writing Tools) follow the same editing rules as
  typing. A multiple selection (Command-drag) becomes its first range.
- **Writing Tools** run in their panel, not inline, and their results
  arrive as ordinary replacements.

## Key bindings

Command is the main modifier and Option the second.

| Command | Keys |
| --- | --- |
| New, Open, Save, Save As | Cmd+N, Cmd+O, Cmd+S, Cmd+Shift+S |
| Print, Close, Quit | Cmd+P, Cmd+W, Cmd+Q |
| Enter Full Screen | Ctrl+Cmd+F |
| Find, Find and Replace | Cmd+F, Cmd+Option+F |
| Find Next, Find Previous, Use Selection for Find | Cmd+G, Cmd+Shift+G, Cmd+E |
| Larger, Smaller, Actual Size | Cmd++ (or Cmd+=), Cmd+-, Cmd+0 |
| Show Markdown, Reflow Paragraphs | Cmd+/, Cmd+Option+Z |
| Bold, Italic, Strikethrough, Inline Code | Cmd+B, Cmd+I, Cmd+Shift+X, Cmd+Shift+E |
| Link | Cmd+K |
| Normal Text, Heading 1–6 | Cmd+Option+0, Cmd+Option+1…6 |
| Numbered List, Bulleted List, Checklist | Cmd+Shift+7, 8, 9 |
| Quote, Code Block | Cmd+Option+Q, Cmd+Option+C |
| Indent, Outdent | Tab or Cmd+], Shift+Tab or Cmd+[ |
| Toggle Task | Cmd+Return |
| Open Link | Cmd+click under the pointer; Cmd+Option+Return at the cursor |
| Comment on Selection | Cmd+Option+M |
| Copy Open Comments | Cmd+Shift+C |
| Next Comment, Previous Comment | Cmd+Option+Down, Cmd+Option+Up |
| Reply, Post | Cmd+Option+R, Cmd+Return |
| Leave Comment, close Find | Escape |
| Keyboard Shortcuts | in the Help menu |

Keys macOS gives to something else:

- **Cmd+E** is Use Selection for Find, so Inline Code is Cmd+Shift+E.
- **Option+letter types characters** (Option+Z is Ω), so no command is
  Option-only.
- **Cmd+?** opens the Help menu's search, so Keyboard Shortcuts has no key.

Open question: Cmd+Option+Q (Quote) sits next to Cmd+Q; is that too close?

## Notifications

The Mac has no toast, so a small banner at the bottom of the window (a HUD
material with a 10pt radius) says "Updated from disk", "1 new reply" and the
like, and fades after 3 seconds, or 6 when it has an Undo button. It is also
announced to VoiceOver.

Open question: should agent activity also post a system notification when
Margin is in the background?

## Files and storage

- Comments in `~/Library/Application Support/Margin/docs/`, the Mac's place
  for app data, and drafts beside them in `drafts/`. `XDG_DATA_HOME`, when
  set, and `MARGIN_DATA_DIR` override it, so the CLI and the editor always
  agree.
- Preferences (zoom, Reflow Paragraphs) in the user defaults under
  `io.github.tibbe.Margin`.

## App lifecycle

- One instance. `margin FILE…` creates files that don't exist yet before
  opening them, rather than on first save: Finder only opens existing
  files.
- Margin is an editor for Markdown files in Finder's Open With, and opens
  them on double-click.
- The app keeps running when its last window closes, as Mac document apps
  do; clicking its Dock icon then shows the Open panel.
- Quitting keeps untitled documents and asks, one window at a time, only
  about text whose save failed; Don't Save finishes quitting.

## Distribution

- The app runs on Apple silicon and Intel, and the `margin` CLI ships
  inside it.
- Signed with a Developer ID and notarized, distributed as a disk image or
  through Homebrew Cask, which links the CLI onto the PATH.
