"""Summarize the timings of the frames recorded by `reedi --timing`."""

import json
import sys


def load(path):
    with open(path) as file:
        return json.load(file)


def main():
    timing = load(sys.argv[1])
    refreshes = timing["refreshes"]
    print(f"{len(refreshes)} refreshes")
    print(f"longest: {max(refreshes):.1f} ms")


if __name__ == "__main__":
    main()
