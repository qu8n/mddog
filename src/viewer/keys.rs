pub const KEYS: &str = "Scroll
  j k, Down Up            one row (also Ctrl-e Ctrl-y)
  d u, Ctrl-d Ctrl-u      half a screen
  Space f b, PgDn PgUp    a screen (also Ctrl-b back)
  g or gg, G, Home End    top, bottom
  h l, Left Right         wide code or tables on screen, sideways
  mouse wheel             3 rows (Shift + wheel: sideways)
  Backspace, Ctrl-o       back to where you were before a jump, or to the file before

Mouse
  click a link            open it in the browser, or go to a # heading
  click a button          open or close a <details>, copy code, zoom a diagram, or show it full size
  drag, double-click      select text or a word
  Cmd-c                   copy the selection
  drag a scroll bar       scroll that code block or table sideways
  click the bottom row    run that key

Search
  / or Ctrl-f             search (Enter runs it, Esc cancels)
  n N, Enter              next, previous match
  Esc                     clear the search
  Ctrl-u Ctrl-w           in the prompt: clear it, delete a word
                          Lowercase matches any case. Uppercase matches exactly.

Contents
  t                       show or hide the table of contents
  1-9                     jump to a section, press again for its subsections
  0                       step backwards on the next repeat of a digit
  J K, ]] [[              next, previous section
  D U, wheel over it      scroll the contents list, or drag its scroll bar
  click an entry          jump to that section

Other
  w                       wrap or unwrap long lines in code blocks
  e or v                  edit the file in $EDITOR (default vi)
  c                       copy the file's Markdown source
  ?                       this help. ? or Esc goes back to the page
  Ctrl-p                  list the Markdown files here: type to filter, Enter opens
  Ctrl-z                  suspend to the shell
  q, Ctrl-c               quit";
