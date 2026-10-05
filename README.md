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
- **Changes since the last commit are marked** with a bar in the left
  margin, so you know what to review after an agent's edits.
- **Agents use the `margin` CLI** to list threads with `file:line`
  locations, reply and resolve. Replies appear in the open editor, and when
  an agent edits the document, comments stay attached to their text.

## Install

### Linux

On Omarchy, or any Linux with GTK 4.20+ and libadwaita 1.8+, with Rust:

```sh
./install.sh
```

This installs `margin` into `~/.local/bin`, with a launcher entry and icon.

### macOS

On macOS 26, with Homebrew:

```sh
brew tap tibbe/margin https://github.com/tibbe/margin
brew install --cask tibbe/margin/margin
```

This installs Margin into `/Applications` and the `margin` CLI onto your PATH.

Or build it, with Xcode 26 and Rust:

```sh
xcodebuild -project macos/Margin.xcodeproj -scheme Margin archive -archivePath /tmp/Margin.xcarchive
xcodebuild -exportArchive -archivePath /tmp/Margin.xcarchive -exportPath /tmp/Margin \
  -exportOptionsPlist macos/ExportOptions.plist
ditto /tmp/Margin/Margin.app /Applications/Margin.app
ln -sf /Applications/Margin.app/Contents/Helpers/margin ~/.local/bin/margin
```

The last line puts the `margin` CLI on your PATH; any directory on it will
do.

### Coding agents

Teach your coding agent the workflow:

```sh
npx skills add tibbe/margin
```

## License

MIT; see [`LICENSE`](LICENSE). Margin was written by Claude, in Claude Code,
directed by Johan Tibell.
