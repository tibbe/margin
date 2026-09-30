# Linux editor

- Test the UI headlessly, on a Broadway display:

  ```sh
  gtk4-broadwayd :7 &
  GDK_BACKEND=broadway BROADWAY_DISPLAY=:7 MARGIN_DATA_DIR=/tmp/margin-test \
    MARGIN_SCRIPT=steps.txt margin doc.md
  ```

  The script language (type text, press keys, add comments, take
  screenshots) is documented at the top of `crates/gtk/src/debug.rs`.
  `tools/run-ui-script.sh SCRIPT DOC` does the same with real mouse clicks
  and key presses sent through the display; test anything the mouse does
  that way.
