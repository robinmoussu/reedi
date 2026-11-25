def wrap(line, width):
    """Split `line` into rows of at most `width` columns."""
    rows = []
    while len(line) > width:
        rows.append(line[:width])
        line = line[width:]
    rows.append(line)
    return rows
