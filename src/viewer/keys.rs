pub const KEYS: &str = "Scroll
  j k, Down Up            scroll down, up one row (also Ctrl-e Ctrl-y)
  d u, Ctrl-d Ctrl-u      scroll down, up half a screen
  Space f b, PgDn PgUp    scroll down, up a full screen (also Ctrl-b up)
  g or gg, G, Home End    go to the top, the bottom of the file
  h l, Left Right         scroll wide code blocks and tables on screen left, right
  mouse wheel             scroll 3 rows. Shift + wheel to scroll a code block or table sideways
  Backspace, Ctrl-o       go back to where you were before a jump, or to the file before

Mouse
  click a link            open it in the browser, or for a # link, scroll to its heading
  click a button          copy a code block, open or close <details>, zoom or expand a diagram
  drag, double-click      select text, select a word
  Cmd-c                   copy the selected text
  drag a scroll bar       scroll that code block or table sideways
  click the bottom row    run the key named where you click

Search
  / or Ctrl-f             open the search prompt (Enter to search, Esc to cancel)
  n N, Enter              go to the next, previous match
  Esc                     clear the search, or close the contents panel
  Ctrl-u Ctrl-w           in the prompt: clear the text, delete the last word
                          All lowercase ignores case. Any uppercase letter makes case count.

Contents
  t                       show or hide the contents panel
  1-9                     jump to the Nth section. Press again to step through its subsections
  0                       make the next repeated digit step backwards through subsections
  J K, ]] [[              go to the next, previous section
  D U, wheel over it      scroll the list in the contents panel (or drag its scroll bar)
  click an entry          jump to that section

Other
  w                       wrap or unwrap long lines in code blocks
  e or v                  open the file in $EDITOR (default vi), then reload it
  c                       copy the file's Markdown source to the clipboard
  ?                       show this list. Press ? or Esc to go back to the page
  Ctrl-p                  list Markdown files in this repo or folder: type to filter, Enter to open
  Ctrl-z                  pause mddog and go back to the shell (fg to resume)
  q, Ctrl-c               quit mddog";
