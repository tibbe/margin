#!/bin/sh
# Runs Margin headlessly on a GTK Broadway display with a test script (see
# crates/gtk/src/debug.rs), with tools/broadway-click.py connected so that `sh echo
# "click X Y" > "$MARGIN_CLICK_FIFO"` steps send real mouse clicks.
#
#   tools/run-ui-script.sh SCRIPT DOC [DISPLAY]
set -eu
script=$1 doc=$2 display=${3:-7}
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
export MARGIN_CLICK_FIFO="$tmp/click.fifo"
gtk4-broadwayd ":$display" >/dev/null 2>&1 &
broadwayd=$!
sleep 0.5
python3 "$here/broadway-click.py" --display "$display" serve "$MARGIN_CLICK_FIFO" &
clicker=$!
sleep 0.3
status=0
GDK_BACKEND=broadway BROADWAY_DISPLAY=":$display" MARGIN_DATA_DIR="${MARGIN_DATA_DIR:-$tmp/data}" XDG_CONFIG_HOME="$tmp/config" \
  MARGIN_SCRIPT="$script" timeout 60 "$here/../target/debug/margin" "$doc" || status=$?
kill "$clicker" "$broadwayd" 2>/dev/null || true
rm -rf "$tmp"
exit "$status"
