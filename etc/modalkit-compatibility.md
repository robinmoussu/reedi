What `modalkit` doesn't provide yet
===================================

`reedi` parses the keys of `--send-keys` with the parser of Vim's key notation of
[`modalkit`](https://github.com/ulyssa/modalkit), which is only available inside the whole
`modalkit` crate, and whose keys can't be converted into the key events of `crossterm`.

The `modalkit` submodule is a fork, on the `modalkit-key` branch of
https://github.com/robinmoussu/modalkit, of `modalkit` 0.0.32 (commit `da2e8d3`). Each of its
commits is meant to be proposed upstream. When `modalkit` provides the same, the submodule should
be replaced by the published crate.

The `modalkit-key` crate
------------------------

- Commit: `ddb56f9` "Extract the key module into the modalkit-key crate"
- Missing in `modalkit`: the `key` module (`TerminalKey` and its parser) is part of `modalkit`,
  which depends on much more than the parser needs
- Changed: `crates/modalkit/src/key` moved to `crates/modalkit-key`, re-exported by `modalkit`; `parse` and
  `TerminalKey::get_char_mods()` made public

From `TerminalKey` to `crossterm`'s `KeyEvent`
----------------------------------------------

- Commit: `4ac42fc` "Convert TerminalKey into crossterm's KeyEvent"
- Missing in `modalkit`: the fields of `TerminalKey` are private, and there is no conversion into
  `KeyEvent`
- Changed: `From<TerminalKey> for KeyEvent`, a character pressed with Control being lowercase, as
  terminals report it (`<C-d>` is `d` with Control)
