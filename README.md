reedi
=====

`reedi` is a review editor

> Current implentation status is described in `implementation_status.md`

- It is a TUI tool to review unpushed commits.
- Commits are displayed (with their diff) in topological order (oldest first).
- The general interface is the same than `delta`, but commits messages, author, date and hunks are
  editable.
- All chunks have syntax and diff-aware coloration (including word-level diff), just like what
  `delta` does
- When saving all changes are applied, as-if an interactive rebase was done (note: maybe using `jj`
  instead of `git rebase` would lead to a better UX).
- In review mode the closest editable area is highlighted and the cursor visible (if the cursor is
  on an editable area, this one, otherwise the next one)
- In visual mode (selection) the current selection is highlighted
- If the terminal size change, the UI is updated automatically
- If available, the kitty terminal protocol is used
- Like less, it uses the alternate screen
- Only support utf-8, if data is invalid an error message describing the issue is shown
- Non-latin scripts are expected to be properly supported (best effort, I don’t read them,
  especially right-to-left scripts, but it’s a goal). For RTL scripts, it is expected than a
  terminal that support bidirectional algorithm is used (like GNOME VTE or Konsole).
- Supports `.gitattributes`
- Binary file only show a message if they differ. Non-valid utf8 files are treated the same.
- Likewise commit message, author name and other metadate must be valid utf8 or are replaced by an
  error message.
- Read git configuration, and re-use its settings

```
$hash $commit_subject $author $date ($tags, $branches)

$commit_message_body

$file
───────────────────────────────────────────────────────────────────────────────
│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after

│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after

$file
───────────────────────────────────────────────────────────────────────────────
│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after

───────────────────────────────────────────────────────────────────────────────
$hash $commit_subject $author $date ($tags, $branches)

$commit_message_body

$file
───────────────────────────────────────────────────────────────────────────────
│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after

│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after
│ $line │ $before                       │ $line │ $after

───────────────────────────────────────────────────────────────────────────────
$status_line
```

For exact template and colors, inspiration should be taken from `delta` (except for chunk header `@@
-a,b +c,d @@` which is not shown.

Areas
-----

- `$hash` (short form) is non-editable
- `$commit_subject` is editable
- `commit_message_body` is editable
- `$author` (author name) and `$date` (author date, using current git date format) are editable
- `$line` is non-editable and automatically updated both in the before (left) and after (right). The
  width of this field fit the largest number of that chunk.

- The general idea is to have a navigation mode, in which you can read the diffs, execute commands
  and navigate between the editable areas.
- Editable areas can either be modified using the embeded editor that can use vim (and ideally
  ibm/emacs/helix) style keybinding, or using an external editor (`$VISUAL`, or `EDITOR as
  fallback`).
- The diffs splits the screen 50/50 (right bigger if odd)
- A status bar display the current mode and each mode has an accent color for the background of the
  name of the current mode (`Review`: blue, `Edit`: light green, `Search`: honey, `Comment`: dark
  green, `Command`: mango, `Selection`: purple, `Move`: dark red)
- After the mode, the status bar displays in gray
    - default: `commit $x/$y: $short_hash $summary` (`x`: index of the commit the cursor is on, `y`:
    number of commits displayed)
    - while a number is pending: `The next command will be repeated <n> times`

Expected implementation
=======================

The software is mainly a lib, with a slim binary who only read the config an command line options.

Syntax highlighting
-------------------

- The language is detected from the file name like `delta`: if the path has an extension or the
  file name is longer than 4 bytes, the whole file name then the extension are looked up as an
  extension. The content of the file is never inspected. If nothing matches, the file is plain text
  (no highlighting).
- Themes are the same as `delta`’s default:
    - dark: `Monokai Extended` (dark)
    - light: `GitHub` (light)
    - none: black and white only

Interactivity
-------------

Saving and applying modifications:

- When starting `reedi` the current git state is saved
- Each time the `save` command is executed, this new state is save
- It is always possible to undo all changes and restore the last saved state
- All modifications are immediately applied when leaving an editable area (but this state is not
  saved)
- When exiting `reedi`, if the saved state and the current git state don’t match, there is a
  confirmation dialog
- The default mode is the review mode

Cursor:

- When starting the application, the cursor is on the first row
- The cursor moves row by row: a line displayed on several rows (see "Line wrapping") counts as
  several rows
- The position of the cursor is made visible by highlighting the background of the current row
  (with a dim gray color)
- `$commit_subject`, `$author` and `$date` are on the same line. `right` and `left` is the only way
  to edit `$author` and `$date`, since the cursor is row-based. `right`/`left` Can therefore select
  which of those 3 field is active, and is highlighted when the cursor is on those lines. `j`/`k` do
  not stop on `$date` and `$author` (unless there is line wrapping) and reset to `$commit_subject`
  each time they leave and go back the line containing the subject.
- When resizing, the cursor screen position stays the same. The cursor goes on the row displaying
  the character that was the first of its row: the one of the right half of a diff if this half
  displayed text on that row, otherwise the one of the left half. For example if a line was
  displayed on one row and is now split in 2, the cursor stays on the first one, and if a line was
  displayed on 2 rows, now merged, and the cursor was on the second one, the merged row goes at the
  position of the cursor.

Review mode:

- Invisible characters are displayed with a replacements character (trailing whitespace are always
  displayed, even if the line is wrapped)
- TODO: how is non-linear history displayed

- `j`/`k` or `up`/`down` move the cursor one row. The screen automatically move so that there is
  always 5 extra rows on top/bottom (unless the beggining/end of the file is reached)
- `tab`/`shift+tab` or `left`/`right` move the cursor to the next/previous editable area (this does include
    `commit_subject`, `author` and `date` that are on the same line)
- `g`/`G` move the cursor to first/last line
- `J`/`K` or `ctrl+down`/`ctrl+up` jumps to the next/previous commit
- `[`/`]` jumps to the next/prevous merge conflict

- `ctrl+e`/`ctrl+y` move the screen one line down/up (and move the cursor if it would have gone
  closer to 5 lines of the top/bottom)
- `d`/`u` move the half a screen down/up (and may move the cursor like `ctrl+e`/`ctrl+y`)

- numbers before a move repeat that move by this amount
- `${number}g` move the cursor to the `number`th line from the top, `${number}G` from the bottom

- `enter` edit the current editable area (or the next one if on a line with only non-editable
    elements
- `ctl+enter` (or `alt+enter`) edit the current editable area (or next if on a non-editable area)
  with `$VISUAL` (or `$EDITOR` if unset)
- `c` enters comment mode / `C` with the external editor

- `/` starts search mode
- `?` starts search mode in reverse order
- `n`/`p` jumps to the next/previous item matching the search pattern
- `ctrl+l` clear the screen including all search results

- `ctrl+z` put the application in the background
- `q`/`ctrl+c` quit after a confirmation dialog if the currently saved state doesn’t match the
  current state
- `Q` undo all changes (after a confirmation dialog) and quit
- `w` or `ctrl+s` save the current git state (by default the state when opening `reedi`)
- `ctrl+r` undo all changes and restore the saved git state after a confirmation dialog

- `v` starts visual mode (available only on a chunk diff)
- maintaining `shift` and pressing `up`/`down` or `j`/`k` select those lines (like what the visual
  mode does), and when `shift` is released comment mode is openned on those lines. Same behavior
  with click+drag with the mouse

- `m` start move move (available only on a chunk diff). The moved span is all modified lines of the
  current chunk
- In visual mode, `m` start move mode, and the span are the selected lines

- `b`/`e` adds lines at the beggining/end of the current chunk (possibly merging it with adjacent
- `space` toggle between expanding all chunks of the current file and restoring their default state

- `!` enters command mode

Selection (visual mode):

- selection can only be done in a single file, with a continuous range of lines. TODO: Until `b`/`e` are
  implemented, it is therefore limited to a single chunk. Once those will be implemented, trying to
  extend a range outside a chunk expand the chunk alongside the selection.
- `up`/`down` or `j`/`k` extend the selection in that direction
- `c`/`C` and `enter`/`ctrl+enter`(or `alt+enter`) enters comment mode with the embeded/external
   editor
- `escape` return to review mode. This is also applies to `shift`-selection and drag-selection
- `o` switch to the other side of the selection (ie. next `up`/`down` will expand/shrink from the other side)

Comment mode:

- Comment mode open the embeded (or external) editor.
- Each comment is associated with a span. If comment mode was openned with `c`/`C` it is the whole
  highlighted area. If it was openned after a selection, the span is the span of the
  selection
- A non-empty comment create a new editable zone, clearing all its content remote it
- When exiting comment mode the comment file is updated. TODO: specify the format, but something
  like `file:begin-end\nindented multi-line comment` should work, more probably a `.typ` file
- When exiting `reedi` or saving, the human-readable comment file is updated. It should probably be
  a pdf generated with typst, that embeed the before/after state of the code + a few (3?) lines of
  context + the comment

Search mode:

- It’s a one-line editable area inside the status bar
- It use perl regex, in smartcase by default
- an initial `\I` force to ignore case, `\C` to match the case, `\S` to use smartcase
- an initial `\R` force regex use, `\L` an literal search
- both of the above settings can be combined. If the first character of the search pattern is a `\`
  it should be doubled
- search is incremental, highlight all results, and automatically move the screen to the
  next/previous match (if it was started with `/` or `?`)
- Search ignore invisible characters replacement

- `enter` save the current search pattern and return to review mode
- `escape` doesn’t save the current search pattern, move back the cursor and screen position to what
  it was before the search
- `up`/`down` restore the previous/next search pattern
- default emacs style edition works: `left`/`right`, `ctrl+w`/`alt+d`, `ctrl+b`/`ctrl+f`,
   `alt+b`/`alt+f`, `ctrl+d`/`ctrl+h`, …
- `alt+e` open the search pattern in the external editor

Command mode:

- The interface is the same as the search mode
- `enter` will run `$command` at the current commit (as-it `exec $command` had been used in
  `git rebase --interactive` after the current commit).
- The output of the command is displayed on the full screen, usual pager command (`j`/`k`,
  `ctrl+d`/`ctrl+e`,  …) should work and `q` or `enter` close it.
- If running that command modifies the working directory, the view is refreshed exactly as-if those
  modifications had been made using the editor (and the undo tree of the modified files is updated
  accordingly with a single new entry per area)

Edit mode (embeded editor):

- Invisible characters are displayed normally (no replacements character)
- Right-to-left text is reordered and joined by the terminal. The cursor moves in memory order, and
  is displayed on the character it is on
- Copy interact with the system clipboard. In non-vim mode, `ctrl+c` copy
- Each editable area has a separate undo tree
- This undo tree is associated with the patch id of the associated commit, and thus re-loaded
  automatically between sessions
- `ctrl+escape` (or `alt+escape`, or `escape,escape` for old terminals) exit edit mode (reset to
  review mode)
- If the current edition mode is using vim-style keybinds, the vim mode is displayed in the status bar
- `author` and `date` are validated. During edition, if invalid, with a red background, when exiting
  edit mode, if still invalid, they are reset to the value before edition (e.g. multiline or invalid
  date format).
- When modified, `date` (if valid) is re-formated using the current git date format preference
- When editing the `author` field `author <email>` should be used (with autocompletion for the list
  of recent authors), but only the author name is displayed. When entering edit mode, the author
  name is replaced by `author <email>`
- when entering edit mode, the cursor is put at the beggining of the line the cursor was in review
  mode. When exiting edit mode, the cursor stays on the same line.

Merge conflicts:

- Merge conflicts are displayed with the `<<</===/>>>` chunks, and are expected to be fixed
- A merge conflict indicator is displayed in the status bar
- `[`/`]` jumps to the next/prevous merge conflict
- TODO: maybe using `jj` would help for merge conflicts

Move mode:

- `k`/`j` and `up`/`down` move the current span to the previous/next commit
- in move mode, all files/chunks are hidden, except for the current span and its surrounding context

Terminal protocol:
- Use kitty protocol by default, with a fallback for non-kitty-aware terminals
- A left-to-right mark (U+200E) is put after the first character of each row and after the last
  character of each column. Terminals reordering bidirectional text do it for the whole row, so
  without it, the punctuation and numbers next to a right-to-left text (like Persian) would be moved
  with it, even from another column

Line wrapping
-------------

The text area of a row is the columns of that row in which the text of a line is displayed. For the
header, the commit message and the file names, it is the whole row. In a diff, there is one text
area per half: the columns after its line number column, up to (excluding) the column reserved for
the `↵` marker.

A line too wide for its text area is displayed on several rows. A continuation row is a row
displaying the part of a line that did not fit on the previous row.

- Lines are broken at the break opportunities of the Unicode line breaking algorithm (UAX #14), as
  late as possible: for example at spaces and after a hyphen, but not between `.` and a letter. A
  word (the text between two break opportunities) wider than the text area is split after its last
  grapheme cluster that fits.
- Break opportunities are computed on the original text, not on the displayed text (in which the
  invisible characters are replaced). The replacement character of the last character before a
  break is displayed at the end of the row before the break if it fits.
- Widths are display widths: wide characters (like CJK and emoji) count for 2 columns. A wide
  character that does not fit at the end of a row goes to the next row, and the rest of the row
  is left blank.
- Whitespaces that break a line:
    - the spaces and U+2800 around a break are the ones ending the row before it and starting the
      row after it. If there is only one, it is not displayed (its column is left blank), unless it
      is a trailing space
    - otherwise, spaces before a break are displayed with the replacement character. Spaces that
      do not fit the row are moved to the next row (and are displayed as plain space like usual
      unless they are also trailing spaces)
    - U+2800 (braille pattern blank) can be part of the trailing whitespace, like spaces. When it is
      not, it is displayed with the replacement character when it is at the end or at the beginning
      of a row because of a wrap (unless it is the only space or U+2800 around that break), and
      displayed normally elsewhere
    - tabs that are the last character before the break, are displayed as `⇥` alone (without spaces
      up to the next tab stop). After a break they are displayed with the replacement character as
      usual (in all case `⇥` is displayed)
    - the replacement characters of non-trailing spaces, U+2800 and tabs use the dimmed whitespace
      color, and those of trailing spaces, U+2800 and tabs always use the trailing whitespace color
      (including before a break)
    - other whitespaces are replaced and highlighted normally
- Tab stops are counted from the start of the text area
- Continuation rows start at the first column of the text area: the indentation of the line is not
  repeated.
- In a diff:
    - the last column of each half is always reserved for the `↵` marker, which is displayed on
      each row followed by a continuation row of the same line
    - continuation rows display the line number column without number (`│    │`, as wide as the
      line number column of the chunk)
    - if one side of a line wraps on more rows than the other, the shorter side is padded with rows
      displaying only the line number column without number
- The header, the commit message and the file names are wrapped the same way, but without `↵`
  marker. The header can be broken inside any field (like the subject, or between `tag:` and the
  tag name), each part keeping the color of its field. The spaces between the fields are never
  replaced (nor colored as dimmed or trailing whitespace), even next to a break.
- Right-to-left text is broken in memory order, like any other text, and then reordered by the
  terminal (see the beginning of this document). The right-to-left part of a text area can thus be
  displayed on either side of its left-to-right part. Rows are still left-aligned.
- A text area too narrow to display a grapheme cluster of its line (0 column, or 1 column for a wide
  character) cannot be wrapped. In a diff, a clear error message is displayed in place of the
  chunk. Elsewhere, or if this error message cannot be displayed either, `reedi` exits with a clear
  error message.

Settings
--------

- All settings can be configured in `$XDG_CONFIG_HOME/reedi/config.toml` (if unset:
  `~/.config/reedi/config.toml`)
- A missing file, or a file without the preference key, means the preference is unset
- An unreadable file, an invalid TOML file, or an invalid value makes `reedi` exit with a
 clear error message
- A warning is displayed if an unknown key is used (displaying both the faulty key and its value)

- There is a named action for each of them for each shortcut, and they can be all remapped (and
  unmapped).
- Incremental search can be toggled off
- Scrolloff (by default 5) can be changed
- external editor (by default `$VISUAL`, then `$EDITOR`) can be changed
- seach case setting can be set to smartcase (default), ignorecase, and noignorecase
- search pattern setting can be set to perl regex (default) or literal
- embedded editor style can be set to vim (default) / emacs / standard / helix
- tab width is 4 spaces by default
- theme has light, dark, and none variant (terminal theme by default, dark otherwise)
- each invisible character replacements can be configured. Default:
    - tab: `⇥`, followed by spaces up to the next tab stop
    - space, when trailing or before a wrapping break: `∙`
    - U+2800, braille pattern blank, when trailing or before or after a wrapping break: `∙`
    - U+2800, other position: no replacement
    - note: unless it is trailing, the only space or U+2800 around a wrapping break is not displayed
      (its column is left blank), whatever its replacement
    - non-ASCII spaces (U+00A0, U+1680, U+2000–U+200A, U+202F, U+205F, U+3000): `⍽`
    - control characters (U+0000–U+001F), except tab and line feed: their control picture, from `␀`
      to `␟` (U+2400 + their code)
    - carriage return: `␍`, only if a version of a file mixes line endings, or if the before and
      after versions of a file use different line endings
    - delete (U+007F): `␡`
    - line separators (U+2028, U+0085) and paragraph separator (U+2029): `␤`
    - bidirectional text controls (U+202A–U+202E, U+2066–U+2069, U+200E, U+200F, U+061C): `⇄`
    - zero-width characters (U+200B, U+200D, U+2060, U+FEFF), soft hyphen (U+00AD), U+180E, U+034F,
      variation selectors (U+FE00–U+FE0F, U+E0100–U+E01EF) and tag characters (U+E0000–U+E007F):
      `⌁`. The zero-width joiner (U+200D), variation selectors and tag characters are kept when they
      are inside a grapheme cluster (what is displayed as a single character) that starts with an
      emoji. The zero-width non-joiner (U+200C) is never replaced
    - any other invisible character (control characters U+0080–U+009F, Hangul fillers U+115F,
      U+1160, U+3164 and U+FFA0, and the other characters with the Unicode property
      `Default_Ignorable_Code_Point`): `⍰`
    - ZWNJ is not replaced (and thus not colored)
- color of each invisible character replacements category:
    - dimmed whitespace: dim gray
    - trailing whitespace: bold red
    - all other invisible characters: bold red
- error color: bold bright red
- current mode accent color:
    - Review: blue
    - Edit: light green
    - Search: honey
    - Comment: dark green
    - Command: mango
    - Selection: purple
    - Move: dark red
- number of context lines: 3 by default

Colorscheme
-----------

The colorscheme follows the current git colorscheme used by delta.
- The line between commits is red (TODO: make it configurable, ideally re-use something from git
  colorscheme that would make sense)
- `$hash`, `$author`, `$date`, $tags and `$branche` uses git coloration
- `$file` and the line just below share the same color (blue)
- The `↵` marker of wrapped lines has the same color as `$line` (blue)
- Replacements characters of invisible characters have 3 catégories:
    - dimmed whitespace: non-trailing spaces before a line wrap, non-trailing U+2800 at the end or
      at the beginning of a row because of a line wrap (except the only space or U+2800 around a
      line wrap, which is not displayed), and non-trailing tabs (including before a line wrap)
    - trailing whitespace (including U+2800, whitespace-only lines, and before a line wrap)
    - all other invisible characters
