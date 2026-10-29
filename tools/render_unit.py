#!/usr/bin/env python3
"""Render the installed executable path without shell or systemd expansion."""

import argparse
from pathlib import Path


def render(executable):
    if not Path(executable).is_absolute() or any(
        ord(c) < 32 or ord(c) == 127 for c in executable
    ):
        raise ValueError(
            "The executable path must be absolute and contain no control characters"
        )
    escaped = (
        executable.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("%", "%%")
        .replace("$", "$$")
    )
    template = Path(__file__).resolve().parents[1] / "packaging/mf-service.service.in"
    return template.read_text().replace("@EXECUTABLE@", '"' + escaped + '"')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.write_text(render(args.executable))


if __name__ == "__main__":
    main()
