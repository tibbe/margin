# Margin on Linux: design system

The Linux editor is built for [Omarchy](https://omarchy.org) with GTK 4
(4.20+) and libadwaita (1.8+). The goal is for Margin to feel like one of
Omarchy's own apps (such as Omawrite).

## Foundations

### Look

Omarchy's look: **square, flat, muted.**

- No rounded corners anywhere: windows, popovers, dialogs, buttons, cards.
  Omarchy's rounding setting only reaches windows and its shell, so this is
  hard-coded, as in Omarchy's own apps.
- No shadows on header bars, buttons, popovers or dialogs. Popovers and
  dialogs get a 1px muted border instead; popovers have no arrow.
- Borders are 1px in the theme's muted color.
- The accent color marks only what has focus: the focused comment card's
  border, a focused text box's border, the caret, and hover on the gutter's
  comment button.
- No avatars or author names.
- Header bars use the window background, so the chrome disappears into the
  page.

### Color

Margin follows the current Omarchy theme, live, when there is one: it reads
`~/.local/state/omarchy/current/theme/colors.toml` and watches the theme
folder for Omarchy's theme switches (`MARGIN_THEME_DIR` overrides it, for
tests). The theme's mode (or its background's luminance) decides light or
dark, and libadwaita is forced to match.

| Role | Omarchy key (first found) | Adwaita light | Adwaita dark |
| --- | --- | --- | --- |
| Background | `background`, `bg` | `#ffffff` | `#1e1e1e` |
| Text | `foreground`, `fg` | `#1f1f1f` | `#e8e8e8` |
| Headings | `bright_foreground`, `bright_fg` | `#000000` | `#ffffff` |
| Dimmed text (syntax, times, H6) | `dark_foreground`, `dark_fg`, `muted` | `#6f6f6f` | `#9a9a9a` |
| Borders | `muted`, `dark_foreground` | `#d0d0d0` | `#4a4a4a` |
| Accent | `accent`, `blue` | `#1c71d8` | `#78aeed` |
| Selection | `selection`, `selection_background`, `lighter_background` | `#c8ddf7` | `#2f4a6e` |
| Code background | `lighter_background`, `dark_background` | `#f3f3f3` | `#2a2a2a` |
| Card background | `lighter_background`, `dark_background` | `#fafafa` | `#2a2a2a` |
| Comment highlight | `yellow`, `bright_yellow`, `orange` | `#f6d32d` | `#f8e45c` |
| Links | `blue`, `accent` | `#1c71d8` | `#78aeed` |

Missing Omarchy keys fall back to the text, background or accent color.
Without an Omarchy theme, Margin uses the Adwaita palette above and follows
the desktop's light or dark preference.

Derived colors: quotes are text at 78% opacity; strikethrough text at 65%;
checked tasks at 50%.

### Type

- **Body**: the desktop's document font (GNOME's `document-font-name`, which
  Omarchy sets), default Adwaita Sans 12pt. `MARGIN_FONT` overrides it.
- **Code**: the generic `monospace` family, which fontconfig resolves to the
  font `omarchy font set` picks. `MARGIN_MONO_FONT` overrides it. Code blocks
  and tables (shown as source, not yet as grids) are at 88% of body size,
  inline code at 90%, fences at 80%.
- **Interface** (comment cards): the desktop's interface font
  (`gtk-font-name`).
- **Text scaling**: Omarchy's text size sets GNOME's text-scaling-factor,
  which GTK applies to every font. Margin follows it live, and follows font
  changes live.
- Heading weights, H1 to H6: 700, 700, 700, 650, 650, 600.

### Geometry

At a 16px (12pt) body, scaled with the text size:

- Text column up to 760px (about 100 characters), at least 300px, centered,
  with 28px side padding.
- Comment cards 300px wide (scaled with the desktop's text scaling only),
  40px from the text. Cards stack with 10px between them.
- Default window 1340×920.

### Feedback

- **Notifications** are libadwaita toasts at the bottom of the window. Undo
  is a toast button.
- **Dialogs** are `AdwAlertDialog`s.

## Files

- **Header bar** (`AdwHeaderBar`): the file name as title (prefixed with `•`
  while unsaved) and its folder as subtitle (`~/…`, or "Not saved yet" for
  untitled documents).
- The window manager's title is "name – Margin", for task switchers and the
  bar.
- Dialogs: the conflict dialog (Keep My Version, Load Disk Version as
  destructive) and Unsaved Changes (Cancel, Discard as destructive, Save… as
  suggested).
- Open and Save As use `GtkFileDialog` (the desktop portal), filtered to
  Markdown (`.md`, `.markdown`, `text/markdown`). Open starts in the current
  document's folder.

## Editing

- **Find** is a bar under the header: search field, match count, previous
  and next buttons, an **Aa** toggle for Match case, and a toggle for the
  replace row (Replace with, Replace, Replace All).
- Insert Link or Edit Link is a dialog: a URL field with Cancel, Remove Link
  as destructive, Apply as suggested.
- The pointer becomes a hand over checkboxes, and over links while Ctrl is
  held. A link's tooltip is its URL and "Ctrl+click to open".
- Printing uses GTK's print dialog.

## Comments

- Card background, 1px border, padding 10px by 12px. The focused card's
  border is the accent color. Resolved cards are at 70% opacity. A detached
  thread's quote is struck through.
- Times and "Resolved …" in dimmed text at 85% size; quotes dimmed, italic,
  90%. Replies are separated by a 1px rule.
- In the header row, the author is bold at 85% size; the buttons are a
  Resolve (or Reopen) icon button and a More menu with Delete thread.
- The reply box and the draft's text box have a 1px border that turns the
  accent color while focused. Buttons: Comment or Reply, and Cancel.
- The gutter's comment button appears beside the top of a selection, with a
  card background and border.
- On the right of the header bar: the open-comment count ("3 open
  comments", "2 resolved"), Send to Agent (`mail-send-symbolic`), a comment
  button, and the menu button.
- Send to Agent is insensitive unless an agent is waiting and a thread is
  open; its tooltip says which is missing. While the agent works on a send,
  an `AdwSpinner` sits before it. A send says "Sent 2 open comments to the
  agent" in a toast.
- **System notifications** (see the spec for when) are one `GNotification`
  per document, as GNOME Shell doesn't group them: replaced with the running
  totals ("3 new replies, 1 comment resolved") until the window is active
  again, when it is withdrawn. For a single change the body reads `Resolved
  "quote": Done.`. Normal priority, no category, no permission to ask.
  GNOME Shell shows notifications even for the focused app, so the window
  decides.

## Commands

### Menu

One menu, behind the header's menu button (F10), with no menu bar. Rarely
used commands live here rather than in the header.

- New Window, Open…, Save As…, Print…
- Find…, Find and Replace…
- Format ▸ Normal Text, Heading 1–3; Bulleted List, Numbered List,
  Checklist, Quote, Code Block; Bold, Italic, Strikethrough, Inline Code,
  Link…
- Copy Open, Send to Agent, Resolve All, Show Resolved
- Reflow Paragraphs, Show Markdown, Text Size ▸ Larger, Smaller, Reset;
  Fullscreen
- Keyboard Shortcuts, About Margin

Headings 4–6, indent, outdent, Toggle Task, Open Link and the comment
navigation commands are keyboard only.

**Keyboard Shortcuts** is an `AdwShortcutsDialog`; **About** an
`AdwAboutDialog`.

### Key bindings

Ctrl is the main modifier and Alt the second; the platform's conventions
are GNOME's.

| Command | Keys |
| --- | --- |
| New, Open, Save, Save As | Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S |
| Print, Close, Quit | Ctrl+P, Ctrl+W, Ctrl+Q |
| Menu, Full Screen, Keyboard Shortcuts | F10, F11, Ctrl+? |
| Find, Find and Replace | Ctrl+F, Ctrl+H |
| Find Next, Find Previous | Ctrl+G, Ctrl+Shift+G |
| Larger, Smaller, Reset Text Size | Ctrl++ (or Ctrl+=), Ctrl+-, Ctrl+0; also Ctrl+scroll |
| Show Markdown, Reflow Paragraphs | Ctrl+/, Alt+Z |
| Bold, Italic, Strikethrough, Inline Code | Ctrl+B, Ctrl+I, Ctrl+Shift+X, Ctrl+E |
| Link | Ctrl+K |
| Normal Text, Heading 1–6 | Ctrl+Alt+0, Ctrl+Alt+1…6 |
| Numbered List, Bulleted List, Checklist | Ctrl+Shift+7, 8, 9 |
| Quote, Code Block | Ctrl+Alt+Q, Ctrl+Alt+C |
| Indent, Outdent | Tab or Ctrl+], Shift+Tab or Ctrl+[ |
| Toggle Task | Ctrl+Enter |
| Open Link | Alt+Enter; Ctrl+click under the pointer |
| Comment on Selection | Ctrl+Alt+M |
| Copy Open Comments | Ctrl+Shift+C |
| Send to Agent | Ctrl+Shift+Enter |
| Next Comment, Previous Comment | Ctrl+Alt+Down, Ctrl+Alt+Up |
| Reply, Post | Ctrl+Alt+R, Ctrl+Enter |
| Leave Comment, close Find | Escape |
| Undo, Redo, Cut, Copy, Paste, Select All | GTK's: Ctrl+Z, Ctrl+Shift+Z, Ctrl+X, Ctrl+C, Ctrl+V, Ctrl+A |
| Emoji, move focus out of the text | GTK's: Ctrl+., Ctrl+Tab |

## System

### Lifecycle

- One instance per user through `GApplication` with the ID
  `io.github.tibbe.Margin`: a second launch hands its files to the running
  editor over D-Bus.
- `margin FILE…` from a terminal starts the editor detached, in its own
  process group, and returns at once. The launcher runs
  `margin --foreground %F`.
- Launched without files and with no drafts to recover, cancelling the Open
  dialog quits.
- The app quits when its last window closes.

### Storage

XDG base directories:

- Comments: `$XDG_DATA_HOME/margin/docs/` (default
  `~/.local/share/margin/docs/`); each store has a `.lock` file beside it.
- Drafts: `$XDG_DATA_HOME/margin/drafts/Untitled <date> <time>.md`.
- Preferences: `$XDG_CONFIG_HOME/margin/settings.json` (default
  `~/.config/margin/`), with `wrap_paragraphs` and `zoom`. Scripted test runs
  don't write it.
- File changes are watched with GIO file monitors.

### Installation

`./install.sh` builds a release binary into `~/.local/bin/margin` and
installs a desktop entry (`io.github.tibbe.Margin.desktop`: Office and text
editor categories, `text/markdown` MIME types) and a scalable icon under
`~/.local/share`.

### Testing

The UI is tested on a Broadway display with `MARGIN_SCRIPT`, taking
screenshots with the script's `shot` step. Anything a mouse does is tested
with real clicks through `tools/run-ui-script.sh`. See the README's
Development section.
