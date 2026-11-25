"""Summarize the timings of the frames recorded by `reedi --timing`."""

import json
import sys

# The number of slowest frames listed.
SLOWEST_COUNT = 5


def load(path):
    with open(path) as file:
        return json.load(file)


def main():
    timing = load(sys.argv[1])
    refreshes = timing["refreshes"]
    print(f"{len(refreshes)} refreshes")
    slowest = sorted(enumerate(refreshes), key=lambda frame: frame[1], reverse=True)
    for index, duration in slowest[:SLOWEST_COUNT]:
        print(f"frame {index}: {duration:.1f} ms")


if __name__ == "__main__":
    main()
