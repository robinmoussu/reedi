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

version 0.0.x: general structure
----------------------------

- [ ] Display only realistic fake Rust fixed values for a single fake commit, but with real logic to
  fill them (real data model, and real rendering, only the data is fake):
   - [ ] $hash
   - [ ] $commit_subject
   - [ ] $author
   - [ ] $date (using `dd/mm/yyyy` format)
   - [ ] extra header field (tags, branches, what `git log --pretty='%d'` generate). Use `(HEAD ->
         main, tag: v1.0, origin/main)` for the fake data.
   - [ ] $commit_message_body
   - [ ] 2 chunks, same file (both half with fake line numbers), fake file name
- [ ] status bar display the current mode (`Review` only for the moment)
- [ ] Chunks do not have any diff logic, nor line wrapping support the fake data is short enough
   with no unpaired lines
- [ ] no cursor yet
- [ ] No semantic/diff syntax highlighting yet (nor extra symbol like `+/-` on the diff), but other
  colored element are colored (using git default colors)
- [ ] display the screen, crop if too big, wait for any key to be pressed, then exit

version 0.0.x: diff and syntax support
------------------------------------

- [ ] fake data is modified to show new constrains
- [ ] line wrapping support (both left and right side)
- [ ] real diff support (including unpaired lines which have a blank filler on the other side
      without line number) with colors
- [ ] syntax support
- [ ] detect terminal color theme

version 0.0.x: minimal interactivity
----------------------------------

- [ ] add cursor
- [ ] Review mode
   - [ ] scrolling works if all the text doesn’t fit in a single screen
   - [ ] `j`/`k`
   - [ ] `left`/`right`
   - [ ] `q` exit (no confirmation)
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
   - [ ] No syntax highlighting
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
- [ ] Review mode
   - [ ] Chunks should be properly highlighted
     - [ ] syntax aware
     - [ ] diff-aware
- [ ] Embeded editor
   - [ ] Chunks should be properly highlighted (syntax + diff aware)
       - [ ] without modifications (ie. no change since `reedi` was started)
       - [ ] with modifications

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

