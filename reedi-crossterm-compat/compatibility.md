What `crossterm` doesn't provide yet
====================================

Each of these is technical debt: when a new version of `crossterm` provides it, the associated
code should be removed.

Shape of the mouse pointer
--------------------------

- Used by: dragging the separation line between the before and after halves of the diffs
- Missing in `crossterm`: no command sets the shape of the mouse pointer (`OSC 22`)
- Replaced by: `src/lib.rs`, whose `SetPointerShape` command writes `OSC 22` with the CSS name of
  the shape. The terminals that don't support it (like the ones based on VTE, such as
  gnome-terminal) ignore it
