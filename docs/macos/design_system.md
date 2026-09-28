# Margin on macOS: design system

The macOS editor lives in `macos/`. This document records the decisions the
[product spec](../spec.md) leaves to the platform, so that Margin feels like
a Mac app rather than a Linux app ported to the Mac. It follows Apple's
Human Interface Guidelines and the conventions of Apple's own document apps
(TextEdit, Pages, Notes).

Decisions are marked **built** when the editor does them, and **proposed**
when they are still waiting. Open questions are listed with each section.

## Principles

- Use the system's own pieces wherever one exists: the menu bar, standard
  window chrome, sheets, the Open and Save panels, the print panel, system
  colors and fonts. Draw custom UI only for what the Mac has no equivalent of
  (the comment gutter, the drawn Markdown blocks).
- Follow macOS conventions over Linux ones where they differ, even when that
  makes the two editors behave differently; the spec's behavior stays the
  same.
- Omarchy's square, flat look is a Linux decision and doesn't apply here.

## Toolkit

**Built:** AppKit in Swift, over the Rust core through UniFFI
(`crates/ffi`), linked as a static library.

- **`NSTextView` on TextKit 1.** The text storage holds the file's text.
  Hidden syntax is laid out as null glyphs by an `NSLayoutManagerDelegate`,
  and lines that are hidden entirely get zero-height line fragments. TextKit
  2 has no way to hide characters: its layout fragments can skip drawing
  them, but caret movement, hit-testing and selection around them would have
  to be rebuilt, which is a text engine's worth of work.
- **Edits go through the core.** The view overrides the typing, newline,
  delete, tab and clipboard actions, and `shouldChangeText` catches every
  other edit (deleting a word, dragging text, a spelling correction). Each
  becomes a core command whose plan is applied as one undoable step. Plain
  typing and single-character deletes that the core leaves unchanged are
  handed back to `NSTextView`, which keeps its native undo coalescing.
- **Positions are UTF-16 at the bridge.** The core works in UTF-8 bytes;
  `crates/ffi` converts every position, with a fast path for ASCII lines.
  Data sent for every keystroke (style spans, lines) crosses as packed
  bytes rather than records.
- **Styling is incremental.** Lines an edit touched are restyled; other
  lines only when the spans over them change (compared by a per-line
  signature). Drawing touches only the lines being redrawn. Typing costs
  about 10 ms per key in a 2,800-line document (Apple silicon, release
  build), and under 2 ms in a normal one.
- **The page is two views.** The scroll view's document is a page holding
  the text view (the left margin and the text column, so clicking the margin
  places the cursor) beside a gutter view with the comment cards. The cards
  are ordinary views, not text-view subviews, so the gutter has the arrow
  cursor and its clicks never reach the text.
- SwiftUI's `TextEditor` has no editing hooks, so it can't host Margin.

Open questions:

- TextKit 1 is not deprecated, but Apple's new work (Writing Tools inline)
  goes to TextKit 2. A move would be a large project; watch for signs.
- The core re-parses the whole document per keystroke (about 2 ms at 2,800
  lines). Incremental parsing only matters for much larger documents.

## Look

**Built:**

- Standard window with a unified toolbar. The title is the document name,
  the subtitle its folder (`~/…`, or "Not saved yet"), with the system's
  document proxy icon. Documents save themselves, so the edited dot on the
  close button only shows while a save is failing, or on an untitled
  document with text.
- Light and dark follow the system appearance. The system accent color marks
  focus.
- The gutter is the page's margin: one text background behind the text and
  the cards, with no separator. (A comments sidebar, as in Pages, would be a
  different design: a list, not notes beside their lines.)
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

**Built:** system semantic colors, so every appearance, accent and
accessibility setting (Increase Contrast, Reduce Transparency) works.

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

**Built:**

- Body: the system font (SF Pro) at 15pt, before zoom.
- Code: SF Mono (`monospacedSystemFont`).
- Cards: the system font at the regular and small sizes.
- Margin's zoom applies on top.

**Proposed:** a Settings window to pick the body font (SF Pro or New York)
and size.

Open questions:

- Do the spec's heading sizes, tuned with Adwaita Sans, need adjusting for
  SF Pro or New York?
- Should Margin follow a system text size setting, if macOS offers one to
  third-party apps?

## Menu bar

**Built:** the standard menus, with Margin's commands where Mac users look
for them:

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

## Documents

**Built:** Margin manages its own documents rather than building on
`NSDocument`. The spec's saving rules (write only when changed, atomically,
keeping permissions and CRLF; merge outside changes against the last saved
text; ask only on overlap) don't fit NSDocument's autosave and its "changed
by another application" handling.

- Documents save themselves 0.7s after the last change, when their window
  loses focus, and when they close.
- Outside changes are noticed by watching the file and its folder with
  dispatch sources (a rename-replace, as agents and editors save, included),
  and the comment store the same way.
- **Save As…** writes the document under a new name and continues there;
  its comments move along. **Rename…** and **Move To…** move the file itself
  (keeping its metadata), with its comments. **Duplicate** opens an untitled
  copy, comments included. **Revert to Last Opened** is one undoable step.
- Saves replace the file through `NSFileCoordinator` and
  `FileManager.replaceItemAt`, which keeps permissions, Finder tags and
  extended attributes. Sudden termination is allowed except while a save is
  pending.
- Untitled documents live in the data folder's `drafts/` until saved, and
  reopen after a crash or relaunch.
- Windows reopen where they were after a relaunch (state restoration, by
  path), and documents can share a window as tabs; "+" in the tab bar opens
  an untitled document.
- Open Recent works through an `NSDocumentController` subclass that opens
  Margin's own windows.
- The conflict ("Keep My Version", "Load Disk Version") and Unsaved Changes
  prompts are sheets. Unsaved Changes uses the Mac wording: "Do you want to
  save the changes made to “name”?" with Save…, Cancel and Don’t Save.
- A change an agent makes to the file, or a merge, is one undo step ("Undo
  Outside Change"), so earlier steps stay undoable.
- Comment changes (resolving, reopening, deleting, Resolve All) are undoable
  with ⌘Z too; the banner's Undo does the same while it's the latest step.

## Text input

**Built:**

- **No substitutions:** smart quotes, smart dashes, text replacement,
  autocorrect and link detection are off, because the spec requires the file
  to hold exactly what was typed. Inline predictions are on: accepting one
  inserts text like typing does.
- **Spelling** underlines are on, except in code, links and hidden syntax.
  Grammar is off.
- **Input methods** (Japanese, Chinese, dead keys) compose as usual. When a
  composition starts, the cursor first moves to where the core would put
  typed text (after a link, not inside it). While text is being composed it
  is not restyled, so its marked-text underline stays; once committed it is
  analyzed and styled like typed text. Press-and-hold accents replace the
  letter through the core.
- **Every edit goes through the core**, including those AppKit makes itself
  (deleting a word, Transpose, dragging text, spelling corrections, Writing
  Tools): `shouldChangeText` hands them to the editing rules. A replacement
  inside plain text keeps the formatting around it. A multiple selection
  (Command-drag) becomes its first range.
- **Writing Tools** run in their panel (`.limited`): on TextKit 1 they can't
  rewrite inline, and their results arrive as ordinary replacements.

## Key bindings

**Built:** Mac equivalents of the Linux bindings: Cmd for Ctrl, Option for
Alt, and Apple's standard bindings where they exist. Conflicts with macOS
conventions are resolved in favor of macOS.

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
| Normal Text, Heading 1–6 | Cmd+Option+0, Cmd+Option+1…6 (as in Google Docs for Mac) |
| Numbered List, Bulleted List, Checklist | Cmd+Shift+7, 8, 9 (by physical key) |
| Quote, Code Block | Cmd+Option+Q, Cmd+Option+C |
| Indent, Outdent | Tab or Cmd+], Shift+Tab or Cmd+[ |
| Toggle Task | Cmd+Return |
| Open Link | Cmd+click under the pointer; Cmd+Option+Return at the cursor |
| Comment on Selection | Cmd+Option+M (as in Google Docs for Mac) |
| Copy Open Comments | Cmd+Shift+C |
| Next Comment, Previous Comment | Cmd+Option+Down, Cmd+Option+Up |
| Reply, Post | Cmd+Option+R, Cmd+Return |
| Leave Comment, close Find | Escape |
| Keyboard Shortcuts | in the Help menu |

Why these differ from Linux:

- **Cmd+H** hides the app, so Find and Replace is Cmd+Option+F, as in
  Apple's apps.
- **Cmd+E** is Use Selection for Find, so Inline Code is Cmd+Shift+E.
- **Option+letter types characters** (Option+Z is Ω), so Reflow Paragraphs
  can't be Alt+Z, and no command is Option-only.
- **F10 and F11** have no role with a menu bar; full screen is Ctrl+Cmd+F.
- **Cmd+?** opens the Help menu's search, so Keyboard Shortcuts has no key.

Open question: Cmd+Option+Q (Quote) sits next to Cmd+Q; is that too close?

## Notifications

**Built:** AppKit has no toast, so a small banner at the bottom of the
window (a HUD material with a 10pt radius) says "Updated from disk", "1 new
reply" and the like, and fades after 3 seconds, or 6 when it has an Undo
button. It is also announced to VoiceOver.

Open question: should agent activity also post a system notification when
Margin is in the background?

## Files and storage

**Built:**

- Comments in `~/Library/Application Support/Margin/docs/`, the Mac's place
  for app data, and drafts beside them in `drafts/`. `XDG_DATA_HOME`, when
  set, and `MARGIN_DATA_DIR` override it, so the CLI and the editor always
  agree.
- Preferences (zoom, Reflow Paragraphs) in `UserDefaults` under
  `io.github.tibbe.Margin`. Scripted test runs don't write them.

**Proposed:** a Settings window for the body font and size.

## App lifecycle

**Built:**

- One instance, through Launch Services. `margin FILE…` (the CLI) runs
  `open -b io.github.tibbe.Margin FILE…` and returns at once; `--foreground`
  waits for the app to quit. It creates files that don't exist yet, since
  Launch Services only opens existing ones.
- Launched without files, Margin reopens drafts left by a crash, and
  otherwise shows the Open panel.
- The app keeps running when its last window closes, as Mac document apps
  do; clicking its Dock icon then shows the Open panel.
- Quitting keeps untitled documents (they reopen at the next launch) and
  asks, one window at a time, only about text whose save failed; Don't Save
  finishes quitting.
- Printing uses `NSPrintOperation` with the body at 11pt, in the light
  appearance.
- Margin declares itself an editor for Markdown (`net.daringfireball.markdown`),
  so Finder's Open With and double-click can open documents.


## Building and installation

**Built:** `macos/build.sh` builds `macos/build/Margin.app` with cargo and
`swiftc` (no Xcode project): the Rust core as a static library, Swift
bindings generated by UniFFI, the app, the icon's asset catalog (compiled
with `actool`; `macos/tools/render-icon.swift` renders it from the SVG),
and the `margin` CLI in `Contents/Helpers/`. Release builds are universal
(Apple silicon and Intel) with a dSYM, signed ad hoc with the hardened
runtime.

**Proposed:** a Developer ID signature and notarization, distributed as a
disk image or through Homebrew Cask, which links the CLI onto the PATH.

## Testing

**Built:** test builds (`macos/build.sh debug`) include a script driver,
`MARGIN_SCRIPT` (see `macos/Sources/ScriptDriver.swift`); release builds
don't. Keys and clicks are events sent through AppKit's own routing (menus,
key equivalents, hit-testing), not calls into the editor. When macOS won't
let the test activate the app (because you're using another one), the
driver sends shortcuts and clicks where AppKit would. `os-key` posts
keyboard events with only a key code, so the keyboard layout translates
them, dead keys included. Input methods are exercised through the same
`NSTextInputClient` calls they make. Test runs keep their preferences
apart and take no part in window restoration. `macos/tests/run.sh` runs the
scripts in `macos/tests/` and compares their output with what's expected.
