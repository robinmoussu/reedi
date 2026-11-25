def wrap(line, width):
    """Split `line` into rows of at most `width` columns."""
    rows = []
    while len(line) > width:
        end = last_space_before(line, width)
        rows.append(line[:end])
        line = line[end:]
    rows.append(line)
    return rows
