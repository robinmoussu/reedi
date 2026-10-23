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

All git-related parsing/display is done using `gix`, but when `gix` lacks the functionality:
- `git` itself should be used (if fast enough), libc, or code manually re-written in
  `src/gix_compat/`
- all of those should be seen as technical debt, and when a new version of `gix` gains that
  functionality, the associated code should be removed
- each of those should be documented in `src/gix_compat/compatibility.md`
- each commit that introduce new compatibility shim should contain `Added git compatibily shim:`
  followed by a list of short descriptions of each compatibility shim added, one per line

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
    - [x] the preference is `colorscheme = "light" | "dark" | "none"`
- [x] if preference is unset, detect terminal color theme (with `terminal-colorsaurus`, like
      `delta`, dark if it cannot be detected) to choose between the light and dark theme
   - [x] the detection is done once, before entering the alternate screen
- [x] all styles are grouped in a `Theme` (one per theme: dark, light and none), built once and
  given to the rendering. Later versions will fill it from the git configuration and the syntax
  highlighting theme
   - [x] dark: the current colors
   - [x] light: the git colors (hash, author, date, decorations, file name, rules, `↵` marker,
     errors, invisible characters) are the same as in dark. The other colors are the defaults of
     `delta` in light mode, or their light equivalent:
      - removed line background: `#ffe0e0`, added line background: `#d0ffd0`
      - removed line number: red, added line number: green, unchanged line number: `#dddddd`
      - cursor row background: 254
      - dimmed whitespace: 250
   - [x] none: no color, only the terminal default foreground and background (bold is kept). The
     row of the cursor uses reverse video, and the mode name in the status bar uses reverse video
     and bold
- [x] snapshoot testing use the `none` theme, that only use black and white
   - [x] snapshots stay color independent (they only contain the text)
- [x] dedicated unit tests testing for color validate both light and dark themes
   - [x] the existing color tests are run for both the light and dark themes (not duplicated)

version 0.0.6: internal tooling
-----------------------------

- [x] pure refactor, acceptance tests shouldn’t change

First commit containing only the change needed to support proper date parsing:
- [x] parsed as git would, displayed as git would
- [x] full hash (fake data) -> short hash (display) conversion (use 7 char for fake commits)
- [x] all date formats supported by `git commit --date $date` (fake data) -> displayed date
- [x] The binary should have an hidden `--fake` option that display the fake commits. Currently
  it is also the default behavior, but in the future when the real data will be used, this
  option will be useful
- [x] add hidden `--fake-now` (same input format as `git commit --date`) and `--fake-timezone`
  (IANA name or fixed offset) options to set the current date and timezone used to resolve
  relative dates and dates without an offset in the fake data, and to display dates in the
  `relative`, `human` and `local` formats (defaulting to the current date and timezone).
  Snapshots should use those options (or the internal mechanism). Using `--fake-now` or
  `--fake-timezone` without `--fake` is an error
- [x] add a visible `--date-format` option, accepting the same values as `git log --date=`
  (`default`, `iso`, `iso-strict`, `rfc`, `short`, `relative`, `local`, `human`, `unix`, `raw`,
  `format:<strftime>`):
   - the `-local` variant of each format (except `local`), like `iso-local` or
     `format-local:<strftime>`
   - the aliases `iso8601`, `iso8601-strict` and `rfc2822`
   - `auto:<format>`, which is always `<format>`, since the output is always a terminal
   - if unset, the `log.date` setting of the git configuration is used (read like `git log` does:
     from the repository of the current directory, or only the global and system configuration
     outside a repository), and if it is unset too, `human`
   - `format:<strftime>` uses the `strftime` of the C library, with the time locale (`LC_TIME`)
     of the environment, like git
   - snapshots use `format:%d/%m/%Y`, so that they don't change

Second commit with only the restructuring of fake data:
- [x] fake data should be plain text files whose data is read at compile time and embedded in the
  binary
- [x] fake commit data should be in `etc/fake_commits/${reverse_order}_${descriptive_commit_name}/`:
   - `$reverse_order`: index of the commit, starting from `00`, corresponding of the reverse order
     in which commit are displayed (so that most recently added test will get a higher number, and
     be displayed first without renumbering index of existing fake commits)
   - `$descriptive_commit_name`: short descriptive name of the commit content
- [x] each commit folder should contain:
   - [x] a metadata.toml file containing
      - commit hash (full hash), author, mail, date (absolute, as `git log --date=iso`), decorations
        (structured format: maybe empty string for head, and array for local_branches,
        remote_branches, and tags), message subject, message body
      - any relevant toml comments for that fake commit
      - `gitattributes` (using the full name for each file, so before/after can be
        differentiated)
   - [x] all files in the before/after states, named `$id_$state_$filename`, where:
      - `$id`: index of the pair (the order in which they are displayed, starting from `01`)
      - `$state`: "before" or "after" (a "before" file without "after" is a deletion, the reverse is
         an addition, multiple "before"/"after" with the same `id` should fail tests)
      - `$filename`: name of the file in the fake commit (use `__` for path separation)
- [x] `etc/fake_commits/**` is covered by a single glob in `.gitattributes` (`-text -whitespace`)
  and `.editorconfig`, so that git and editors never modify the bytes of the fake data

version 0.0.7: interactivity
--------------------------

- [x] The content of the status bar (after the mode, separated from it by one blank column) is in
  gray:
   - [x] by default it displays `commit $x/$y: $short_hash $summary`, where `x` is the 1-based index
     of the commit the cursor is on and `y` the number of commits displayed (not the number of
     commits of the repository), and it is updated when the cursor moves
      - [x] the separator line between two commits is the first line of the next commit
      - [x] `$summary` is displayed as is (no replacement of the invisible characters), preceded by
        a left-to-right mark (U+200E), so that the right-to-left text of the summary does not move
        the rest of the status bar
   - [x] if the content does not fit, it is cropped at the end of the status bar and its last
     column displays `…`
   - [x] the gray is a `Theme` style: `Indexed(248)` in dark, `Indexed(240)` in light, no color in
     none
   - [x] in light mode, the whole status bar, but the mode, has a light gray background
     (`Indexed(252)`)
   - [x] the snapshots do not contain the status bar (it has its own tests), so that they do not
     change when the commits or the cursor change
- [x] fake data update (3 new commits, displayed first), each very short (one file with one line
  before and one after, in plain ASCII), only their summary differs:
   - [x] a commit whose summary is fully in a RTL language (Hebrew)
   - [x] a commit whose summary is RTL Arabic then LTR
   - [x] a commit whose summary is LTR then RTL Arabic

- [x] Content of the status bar update:
   - [x] while a number is pending (see "number prefix" below), it displays `The next command will
     be repeated <n> times` instead, in gray (`<n>` is the number as typed, without
     its leading zeros; it always says `times`, also for 1)
- [x] `ctrl+e`/`ctrl+y` move the screen one line down/up (and move the cursor if it would have gone
  closer to the top/bottom of the screen than the scrolloff margin)
   - [x] the screen stops at both ends, like `less`
   - [x] the cursor follows the scroll only when it is inside the scrolloff margin; otherwise it
     keeps its distance from the top/bottom of the screen
- [x] `d`/`u` or `ctrl+d`/`ctrl+u` move half a screen (`height / 2` rows, `height` excluding the
  status bar) down/up: both the screen, like `ctrl+e`/`ctrl+y`, and the cursor, like `j`/`k`, move
  by that amount (the cursor does not move more because of the scroll)
   - [x] when the last row (for `d`) or the first row (for `u`) is displayed, nothing moves anymore
   - [x] if the screen can only scroll partially, the cursor moves by the same (shorter) amount
- [x] `g`/`G` move the cursor to first/last line (`G`: last row of the last commit; the screen
  scrolls just enough to show it)
- [x] `J`/`K` or `ctrl+down`/`ctrl+up` jump to the header row of the next/previous commit
   - [x] `K` on a row of a commit goes to its header row, and on its header row goes to the header
     row of the previous commit
   - [x] on the separator line of a commit (which is the first line of that commit, above its header
     row), `K` goes to the header row of the previous commit and `J` to the header row of the
     following commit: with the cursor on the separator line of the commit 5, `K` goes to the header
     row of the commit 4 and `J` to the one of the commit 6
   - [x] nothing happens on the last/first commit
- [x] number prefix:
   - [x] repeats `j`/`k`, `up`/`down`, `ctrl+e`/`ctrl+y`, `d`/`u`, `ctrl+d`/`ctrl+u`, `J`/`K`,
     `ctrl+down`/`ctrl+up`
   - [x] `${number}g`/`${number}G`: 1-based row from the top (like `${number}G` in vim)
   - [x] a number too large goes to the last row (`g`/`G`), or clamps to the last/first commit
     (`J`/`K`)
   - [x] a leading `0` is ignored, `0` alone does nothing
   - [x] any key that is not a digit or a move (`escape` included) cancels the pending number

version 0.0.x: word-diff and syntax support
-----------------------------------------

- [ ] full diff support:
   - [ ] modified lines have their background: solid red/green (always active unlike `delta`)
   - [ ] word-level diff, like `delta`
- [ ] syntax support

version 0.0.x: editable areas interactivity
-----------------------------------------

- [ ] Review mode
   - [ ] `left`/`right`
   - [ ]  `tab`/`shift+tab` (supports number prefix)
   - [ ] the current editable area (or next if on a non-editable area) should be highlighted
      - [ ] $commit_subject
      - [ ] $author
      - [ ] $date
      - [ ] $commit_message_body
      - [ ] each chunks

version 0.0.x: minimal editing
----------------------------

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
---------------------------------------------------------------

- [ ] Display the content than `%h% s%C(green)% an%C(blue)% ad%C(auto)% d%C(auto)%+N%-C()%+b%-C()`
- [ ] the fake data functions of `src/fake.rs` (`all()` and the named ones, like `file_changes()`)
  are `#[cfg(test)]` and `pub(crate)`

version 0.0.x: edition
---------------------

- [ ] Embeded editor
   - [ ] scrolling works if all the text in a single editable area doesn’t fit in a single screen
   - [ ] Applying modifications
     - [ ] modification to an editable area are applied to the last commit (`git commit --amend`)
     - [ ] it should work even if the number of lines in the patch changes
     - [ ] diff are updated in real time

version 0.0.x: real data (multiple commits)
------------------------------------------

- [ ] Display the same content than `$git log $current_branch $current_branch@{upsteam} -p --reverse` assuming HEAD is attached to `$current_branch` and that has its upstream set.
- [ ] Embeded editor
   - [ ] Applying modifications
     - [ ] modification to an editable area are applied to the proper commit
     - [ ] diff of future comits are updated
     - [ ] merge conflicts are properly displayed

