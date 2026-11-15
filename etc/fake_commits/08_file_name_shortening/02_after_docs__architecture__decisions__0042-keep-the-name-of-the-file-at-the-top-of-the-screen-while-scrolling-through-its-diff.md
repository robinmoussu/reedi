# 42. Keep the name of the file at the top of the screen while scrolling through its diff

Date: 2026-09-22

## Status

Accepted

## Context

When the diff of a file is taller than the screen, its name is scrolled away, and the reader
has to scroll back up to know which file they are reading.

## Decision

The name of the file stays at the top of the screen until the last line of the file is scrolled
away. If it does not fit on the rows left above the rule, it is shortened, in this order:

1. the old path of a renamed file,
2. the label (`added: `, `removed: `, `renamed: `),
3. the suffix (` (binary file)`),
4. the folders, then the end of the stem, then the end of the extension, then the end of the
   name, keeping at least 8 graphemes.

## Consequences

The name of the file is always visible, but the rows of the diff below it are hidden while it is
displayed over them.
