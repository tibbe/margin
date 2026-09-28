#!/bin/sh
# Builds Margin and installs it for the current user: the binary to
# ~/.local/bin, plus a launcher entry and icon.
set -eu
cd "$(dirname "$0")"
cargo build --release
prefix="${PREFIX:-$HOME/.local}"
install -Dm755 target/release/margin "$prefix/bin/margin"
install -Dm644 data/io.github.tibbe.Margin.desktop "$prefix/share/applications/io.github.tibbe.Margin.desktop"
install -Dm644 data/io.github.tibbe.Margin.svg "$prefix/share/icons/hicolor/scalable/apps/io.github.tibbe.Margin.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
command -v gtk4-update-icon-cache >/dev/null && gtk4-update-icon-cache -qtf "$prefix/share/icons/hicolor" || true
echo "Installed margin to $prefix/bin/margin"
