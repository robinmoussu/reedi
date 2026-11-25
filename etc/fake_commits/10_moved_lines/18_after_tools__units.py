def parse_size(text):
    """Parse a size like `1.5 MiB`, in bytes."""
    number, unit = text.split()
    return round(float(number) * UNITS[unit])
