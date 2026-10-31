# Design system

## Window

- Light and dark follow the system's appearance. The accent color marks
  focus.
- The title is the document's name and the subtitle its folder (`~/…`
  under the home folder), or "Not saved yet" for an untitled document.
- The toolbar holds the open-comment count ("3 open comments", or "2
  resolved" when none are open), with the agent's state beside it, Send to
  Agent and a comment button. Every other command is in the menus.
- The text column holds about 100 characters, with the comment gutter to
  its right. The gutter is the page's margin: one background behind the
  text and the cards. Spacing and sizes are proportional to the text
  size, except an image's, which follows only the zoom. Zoom is shared by
  all windows and persisted, on top of the platform's text size.
- The body is in a proportional font, code and source shown as source in a
  monospace one, and cards in the interface font.
- Announcements ("Updated from disk", "1 new reply") are a short message
  that goes away by itself, with an Undo button when what it reports can be
  undone.

Colors go by role:

| Role | For |
| --- | --- |
| Background | The page, behind the text and the cards |
| Text | Body text |
| Headings | Headings 1–5 |
| Dimmed text | Syntax and source, times, Heading 6 |
| Borders | Cards, rules, quote bars |
| Accent | Focus, checked boxes |
| Selection | Selected text, a selected image or diagram |
| Code background | Code blocks, inline code, table headers |
| Card background | Cards |
| Comment highlight | Commented text, stronger for the focused thread |
| Find matches | Find matches, stronger for the current one |
| Links | Links, image alt text |
| Change bars | Added, changed and deleted lines |
| Errors | Diagram errors |

## Commands

Every editor has Margin's commands under these names and keys, beside the
platform's standard commands (New, Open, Save, Print, Close, Quit, Undo,
the clipboard, Find and its commands, text size, Full Screen), which keep
the platform's names and keys.

- A command's keys are, in order: the platform's own binding for it, if
  the platform has one; else Google Docs' binding on that platform; else
  Margin's own. A key the platform gives to something else (a system
  shortcut, a standard command, a character it types) stays with that, and
  the command takes a nearby combination. Margin's own commands keep the
  same letter on every platform, under that platform's modifiers.
- List shortcuts (numbered, bulleted, checklist) go by the number row's
  physical keys, as in Google Docs, so they work on any keyboard layout.
- Keyboard Shortcuts lists every shortcut.

Mod is the platform's main modifier (Command on the Mac, Ctrl elsewhere)
and Alt its second (Option on the Mac):

| Command | Keys |
| --- | --- |
| Show Markdown, Reflow Paragraphs | Mod+/, Alt+Z |
| Bold, Italic, Strikethrough, Inline Code | Mod+B, Mod+I, Mod+Shift+X, Mod+E |
| Link | Mod+K |
| Normal Text, Heading 1–6 | Mod+Alt+0, Mod+Alt+1…6 |
| Numbered List, Bulleted List, Checklist | Mod+Shift+7, 8, 9 |
| Quote, Code Block | Mod+Alt+Q, Mod+Alt+C |
| Indent, Outdent | Tab or Mod+], Shift+Tab or Mod+[ |
| Toggle Task | Mod+Enter |
| Open Link | Mod+click under the pointer; Alt+Enter at the cursor |
| Comment on Selection | Mod+Alt+M |
| Copy Open Comments | Mod+Shift+C |
| Send to Agent | Mod+Shift+Enter |
| Next Comment, Previous Comment | Mod+Alt+Down, Mod+Alt+Up |
| Next Change, Previous Change | Mod+Alt+Shift+Down, Mod+Alt+Shift+Up |
| Reply, Post | Mod+Alt+R, Mod+Enter |
| Leave Comment, close Find | Escape |

## Platforms

- Each editor uses its platform's own pieces wherever one exists: menus,
  window chrome, dialogs, file pickers, the print dialog, system colors and
  fonts. It draws custom UI only for what the platform has no equivalent of
  (the comment gutter, the drawn Markdown blocks).
- Where platform conventions differ, each editor follows its own, even when
  that makes the editors differ; the behavior in this spec stays the same.
