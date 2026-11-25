def parse_size(text):
    """Parse a size like `1.5 MiB`, in bytes.

    >>> size = parse_size("1.5 MiB")
    >>> size == 1572864
    True

    An unknown unit is an error:

    >>> parse_size("3 bananas")
    Traceback (most recent call last):
    ValueError: unknown unit: bananas
    """
    number, unit = text.split()
    return round(float(number) * UNITS[unit])
