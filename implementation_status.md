Prototype
=========

The software (named "reedi") is in Rust (rust-version: 1.99, edition: 2024). 3rd party dependencies are prefered
over hand-written code. Everything impremented comes with a test (unit test or integration test,
whichever make the more sense).

- unless specified, config file is unused in the prototype, all settings use their default value
- each version should be as simple as possible to implement what is required, but keep in mind the
future modification needed for next versions. Most notably dependencies should not be changed
between version, just added.
- If the terminal is too small to fit the whole layout, it is cropped (ignore this non-usecase),
  except for the text areas too narrow to be wrapped (see "Line wrapping" in the README)

Diff model
----------

The diff model does not depend on the library used to access git data. A chunk is a list of lines,
each one being either unchanged, removed or added, with the line number of its first line in the
before and after version of the file (like the hunks of a unified diff). All libraries can produce
it.

Syntax highlighting
-------------------

- Highlighting uses `syntect` with the syntaxes and themes from `bat` (packaged by the `two-face`
  crate), and the Oniguruma regex engine (`onig`), like `delta`.
- The before and after versions of each file are loaded in full. Each one is parsed once
  (`ParseState::parse_line`, which computes the parsing state without computing colors), from its
  first line up to the last line of its last chunk (never past it). The parsing state reached at
  the first line of each chunk is used to color that chunk.
- Parsing a file has a 500ms timeout. If it is reached, the chunks of that file are highlighted
  without the parsing state of the file, exactly like `delta` does (highlighting restarts from an
  empty state at each chunk).

version 0.0.0: general structure
------------------------------

- [x] Display only realistic fake Rust fixed values for a single fake commit, but with real logic to
  fill them (real data model, and real rendering, only the data is fake):
   - [x] $hash
   - [x] $commit_subject
   - [x] $author
   - [x] $date (using `dd/mm/yyyy` format)
   - [x] extra header field (tags, branches, what `git log --pretty='%d'` generate). Use `(HEAD ->
         main, tag: v1.0, origin/main)` for the fake data.
   - [x] $commit_message_body
   - [x] 2 chunks, same file (both half with fake line numbers), fake file name
- [x] status bar display the current mode (`Review` only for the moment)
- [x] Chunks do not have any diff logic, nor line wrapping support the fake data is short enough
   with no unpaired lines
- [x] no cursor yet
- [x] No semantic/diff syntax highlighting yet (nor extra symbol like `+/-` on the diff), but other
  colored element are colored (using git default colors)
- [x] display the screen, crop if too big, wait for any key to be pressed, then exit

version 0.0.1: minimal interactivity
----------------------------------

- [x] add cursor (no editable-area highlight support)
- [x] fake data update:
   - [x] Add a second (older) commit with 2 modified files and 1+3 chunks
- [x] Review mode
   - [x] scrolling works if all the text doesn’t fit in a single screen
   - [x] `j`/`k` and `up`/`down`
   - [x] `q`/`ctrl+c` exit (no confirmation)
   - [x] `ctrl+z` put the application in the background
   - [x] all other keys are inactive

version 0.0.2: invisible characters
---------------------------------

- [x] fake data update:
   - [x] an older commit with all invisible characters combination in between normal text
   - [x] a line made only of right-to-left text
- [x] add support for invisible characters
   - [x] tabs are expanded up to 4 spaces (the first char being its replacement char), so that it
     align with columns that are multiple of 4
   - [x] a left-to-right mark is put after the first character of each row and after the last
     character of each column

version 0.0.x: line wrapping
--------------------------

- [x] fake data update, in a new oldest commit, whose lines are wrapped in the 110 columns wide
  snapshots:
   - [x] long lines on both side (including a word wider than its text area, so that it is split)
   - [x] a line with tab at multiple places to validate that tab alignment is valid
   - [x] a line with wide characters (CJK and emoji)
   - [x] a line starting with right-to-left text, and ending with left-to-right script
   - [x] a line starting with left-to-right, and ending with right-to-left text script
   - [x] a line made only of right-to-left text, long enough to be wrapped
   - [x] a line with multiple trailing spaces
   - [x] a line with multiple trailing tab
   - [x] a line with multiple trailing spaces and trailing tab
   - [x] a line with multiple spaces in the middle, at a break
   - [x] a line with a tab followed by multiple spaces, at a break
- [x] line wrapping support (both left and right side)
   - [x] the `↵` marker is in the last column of each half, on each row followed by a continuation
     row
   - [x] on continuation rows, the line number column (`│    │`) is displayed without number
   - [x] if one side wraps on more rows than the other, the shorter side is padded with rows
     displaying only the line number column without number
   - [x] the header, the commit message and the file names are wrapped too (without `↵` marker)
   - [ ] RTL scripts are broken in memory order and reordered by the terminal (validated manually
     using the fake data, the right-to-left part of a text area can be displayed on either side)
   - [x] a tab that is the last character before a break is displayed as `⇥` alone
   - [x] Trailing whitespace are highlighted, wrapped, and not hidden
   - [x] dimmed ∙ on spaces before a break, except if it is the only space or U+2800 around the
     break, which is then not displayed (unless trailing)
   - [x] spaces that don't fit are moved to the next row
   - [x] a tab after a break are shown normally (⇥ plus padding)
   - [x] U+2800 at the end or at the beginning of a row because of a wrap is displayed as a dimmed
     `∙`, except if it is the only space or U+2800 around the break, which is then not displayed
     (unless trailing)
   - [x] a trailing U+2800 is part of the trailing whitespace (bold red `∙`), even without wrap
   - [x] in the header, tab stops are counted from the start of the row (not from the start of each
     field)
   - [x] in the header, the spaces between the fields are never replaced, even next to a break
   - [x] break opportunities are computed on the original text, and the replacement character of
     the last character before a break stays on the row before the break if it fits
- [x] a text area too narrow to display a grapheme cluster of its line:
   - [x] in a diff, a clear error message is displayed in place of the chunk
   - [x] elsewhere, `reedi` exits with a clear error message
   - [x] if the error message displayed in place of the chunk cannot be displayed, `reedi` exits with
     a clear error message
- [x] the cursor moves row by row
- [x] when the width of the screen changes, the cursor stays at the same screen position, on the
  row displaying the character that was the first of its row (the one of the right half of a diff
  if it displayed text on that row, otherwise the one of the left half)

version 0.0.4: diff-aware internal architecture
---------------------------------------------

- [x] fake data update (new oldest commit):
   - [x] full fake files are now used. The diff is computed automatically. Commits stay independent.
     Fake commit information contains the summary, body, author, email, date
   - [x] an addition-only and a deletion-only block of lines (unpaired lines)
   - [x] another file in another language (shell)
   - [x] an added, a deleted and a renamed file, and a file without newline at its end
   - [x] renames are declared explicitly in the fake data (old path → new path), not detected
   - [x] two very short files: a binary file (containing a NUL byte), and a file that is not valid
     UTF-8
   - [x] the fake data already knows which files are binary (in the future it will come from git
     and `.gitattributes`)
   - [x] a file with a missing newline in the before state and not after
- [x] real diff support
   - [x] the diff is computed with `gix` (its blob diff, `imara-diff`), which will also be used for
     the word-level diff and to access git
   - [x] removed and added lines are paired like `delta`
   - [x] line numbers colors: red/green for modified lines, grey for unmodified lines (like `delta`)
   - [x] line content colors: red/green background for modified lines (unlike `delta` where sometime
         the background is not present)
   - [x] unpaired lines have a blank filler on the other side, with `│` borders and an empty line
         number column, like `delta`
   - [x] added, deleted, binary file changed and renamed files use the same kind of display than
         `delta`
   - [x] binary files only display their file name, as `$file (binary file)` (like `delta`), with no
         chunks
   - [x] whether a file is valid UTF-8 is checked by `reedi`. Files that are not only display their
         file name, as `$file (invalid utf-8 content)` (with the error color), with no chunks
   - [x] if only one side is missing the newline at the end of the file, the last lines are paired,
         and the half missing it has an extra row below the last line of both side, displaying `\ No
         newline at end of file` in the error color, with an empty line number column
   - [x] added/deleted files still use the 50/50 split, with an empty side (like unpaired lines)

version 0.0.5: theming
--------------------

- [x] config file:
   - [x] the config file is `$XDG_CONFIG_HOME/reedi/config.toml` (`~/.config/reedi/config.toml`
     if unset)
   - [x] a missing file, or a file without the preference key, means the preference is unset
   - [x] an unreadable file, an invalid TOML file, or an invalid value makes `reedi` exit with a
     clear error message
   - [x] a warning is display if an unknown preference key is used
      - [x] it is displayed at startup in a modal over the commits (instead of the standard error
        output), closed by any key press, which does nothing else
      - [x] its title is `Unknown key in config file: $path`, then each unknown key is on its own
        line: ``line $line: `$key = $value` ``. Long lines are wrapped
      - [x] the modal is centered, as wide as its content (at most the width of the screen), with
        its border with heavy lines in the error style on the default background, and its title in
        the error style in reverse video (bright red background). Its inside has the default
        background
      - [x] inside the border, there is a blank row above and below the content
      - [x] `Press any key to close` is centered in the bottom border, in the error style (the
        modal is at least wide enough to display it)
      - [x] the status bar keeps displaying the current mode
- [x] read theme preference from config file:
    - [x] the preference is `colorscheme = "light" | "dark" | "monochrome"`
- [x] if preference is unset, detect terminal color theme (with `terminal-colorsaurus`, like
      `delta`, dark if it cannot be detected) to choose between the light and dark theme
   - [x] the detection is done once, before entering the alternate screen
- [x] all styles are grouped in a `Theme` (one per theme: dark, light and monochrome), built once
  and given to the rendering. Later versions will fill it from the git configuration and the
  syntax highlighting theme
   - [x] dark: the current colors
   - [x] light: the git colors (hash, author, date, decorations, file name, rules, `↵` marker,
     errors, invisible characters) are the same as in dark. The other colors are the defaults of
     `delta` in light mode, or their light equivalent:
      - removed line background: `#ffe0e0`, added line background: `#d0ffd0`
      - removed line number: red, added line number: green, unchanged line number: `#dddddd`
      - cursor row background: 254
      - dimmed whitespace: 250
   - [x] monochrome: no color, only the terminal default foreground and background (bold is
     kept). The row of the cursor uses reverse video, and the mode name in the status bar uses
     reverse video and bold
- [x] snapshoot testing use the `monochrome` theme, that only use black and white
   - [x] snapshots stay color independent (they only contain the text)
- [x] dedicated unit tests testing for color validate both light and dark themes
   - [x] the existing color tests are run for both the light and dark themes (not duplicated)

version 0.0.x: word-diff and syntax support
-----------------------------------------

- [ ] full diff support:
   - [ ] modified lines have their background: solid red/green (always active unlike `delta`)
   - [ ] word-level diff, like `delta`
- [ ] syntax support

version 0.0.x: minimal interactivity (second part)
------------------------------------------------

- [ ] Review mode
   - [ ] `left`/`right`
   - [ ] the current editable area (or next if on a non-editable area) should be highlighted
      - [ ] $commit_subject
      - [ ] $author
      - [ ] $date
      - [ ] $commit_message_body
      - [ ] each chunks

version 0.0.x: minimal editing
--------------------------

- [ ] status bar displays the current mode (`Review` and `Edit`)
- [ ] line wrapping support when editing the right side
- [ ] Review mode
   - [ ] `enter` enters edit mode (with the embedded editor) in the current (or next) editable area
      - [ ] $author
      - [ ] $date
      - [ ] $commit_message_body
      - [ ] each chunks
- [ ] Embeded editor
   - [ ] With syntax highlighting
   - [ ] `ctrl+escape`, `alt+escape` and `escape,escape` should go back to review mode
     - [ ] with modifications
     - [ ] without modifications
   - [ ] Modification not applied (only stay in memory, visible, but never applied to the working directory or git)
   - [ ] vim integration (using 3rd party crate). Current mode ("edition/insert" or "edition/normal", …) is displayed in the status bar
   - [ ] fake lines numbers are updated if lines are added/removed
   - [ ] adding a new line on the right add a blank filler (without line number) on the left

version 0.0.x: real data (single commit) and syntax highlighting
------------------------------------------------------------

- [ ] Display the content than `%h% s%C(green)% an%C(blue)% ad%C(auto)% d%C(auto)%+N%-C()%+b%-C()`

version 0.0.x: edition
------------------

- [ ] Embeded editor
   - [ ] scrolling works if all the text in a single editable area doesn’t fit in a single screen
   - [ ] Applying modifications
     - [ ] modification to an editable area are applied to the last commit (`git commit --amend`)
     - [ ] it should work even if the number of lines in the patch changes
     - [ ] diff are updated in real time

version 0.0.x: real data (multiple commits)
---------------------------------------

- [ ] Display the same content than `$git log $current_branch $current_branch@{upsteam} -p --reverse` assuming HEAD is attached to `$current_branch` and that has its upstream set.
- [ ] Embeded editor
   - [ ] Applying modifications
     - [ ] modification to an editable area are applied to the proper commit
     - [ ] diff of future comits are updated
     - [ ] merge conflicts are properly displayed

