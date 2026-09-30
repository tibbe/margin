# Margin

A native Markdown editor, for Omarchy and macOS, where you leave Google
Docs-style comments that coding agents read and answer from the command
line.

![Margin with two comment threads](docs/screenshot.png)

- **Markdown is an input mode.** Type `# ` and the line becomes a heading;
  type `**bold**` and you get bold. The syntax disappears and what stays is
  a readable document. The file on disk is plain Markdown, and text you
  didn't touch is never rewritten, so diffs stay small.
- **Comments live in the right margin**, anchored to the text they're on.
- **Agents use the `margin` CLI** to list threads with `file:line`
  locations, reply and resolve. Replies appear in the open editor, and when
  an agent edits the document, comments stay attached to their text.

## Install

On Omarchy, or any Linux with GTK 4.20+ and libadwaita 1.8+, with Rust:

```sh
./install.sh
```

This installs `margin` into `~/.local/bin`, with a launcher entry and icon.

On macOS 26, with Xcode 26 and Rust:

```sh
macos/build.sh
ditto macos/build/Margin.app /Applications/Margin.app
ln -sf /Applications/Margin.app/Contents/Helpers/margin ~/.local/bin/margin
```

The last line puts the `margin` CLI on your PATH; any directory on it will
do.

Then teach your coding agent the workflow:

```sh
npx skills add tibbe/margin
```

## License

MIT; see [`LICENSE`](LICENSE). Margin was written by Claude, in Claude Code,
directed by Johan Tibell.
