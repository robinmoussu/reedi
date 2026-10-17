Prototype
=========

The software (named "reedi") is in Rust (rust-version: 1.99, edition: 2024). 3rd party dependencies are prefered
over hand-written code. Everything impremented comes with a test (unit test or integration test,
whichever make the more sense).

- no configuration for the prototype, they all use their default value
- each version should be as simple as possible to implement what is required, but keep in mind the
future modification needed for next versions. Most notably dependencies should not be changed
between version, just added.
- If the terminal is too small to fit the whole layout, it is cropped (ignore this non-usecase)

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
----------------------------

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

version 0.0.x: line wrapping
--------------------------

- [ ] fake data update:
   - [ ] long lines on both side (including a word longer than half the screen)
   - [ ] an addition-only and a deletion-only block of lines (unpaired lines)
   - [ ] a line with wide characters (CJK and emoji) and a line with a tab
   - [ ] a second file in another language (shell)
   - [ ] an added, a deleted, a renamed and a binary file, and a file without newline at its end
- [ ] line wrapping support (both left and right side)
   - [ ] the `↵` marker is at the right edge of each line that continues on the next row
   - [ ] on continuation rows, the line number column is displayed (`│     │`) without number
   - [ ] if one side wraps on more rows than the other, the shorter side is padded with blank rows
   - [ ] tabs are expanded to 4 spaces before measuring the width of a line
   - [ ] the header and the commit message are wrapped too

version 0.0.x: diff and syntax support
------------------------------------

- [ ] real diff support
   - [ ] removed and added lines are paired like `delta`
   - [ ] unpaired lines have a blank filler on the other side, with `│` borders and an empty line
         number column, like `delta`
   - [ ] word-level diff, like `delta`
   - [ ] colors like `delta` (red/green), no `+`/`-` markers
   - [ ] 3 lines of context
   - [ ] added, deleted, renamed and binary files, and files without newline at their end, are
         displayed exactly like `delta`
- [ ] syntax support (without the saved parsing states, which are only needed for editing)
- [ ] detect terminal color theme (with `terminal-colorsaurus`, like `delta`, dark if it cannot be
      detected) to choose between the light and dark theme

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
- [ ] line wrapping support (both left and right side, including when editing for the right side)
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

