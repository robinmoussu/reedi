import pytest


def test_sizes_are_in_bytes():
    size = parse_size("1.5 MiB")
    assert size == 1572864


def test_unknown_units_are_errors():
    with pytest.raises(ValueError, match="unknown unit: bananas"):
        parse_size("3 bananas")
