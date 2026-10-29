#!/usr/bin/env python3
"""Exercise staged install/removal and unit parsing without starting services."""

import os
from pathlib import Path
import subprocess
import tempfile

from render_unit import render

ROOT = Path(__file__).resolve().parents[1]


def main():
    with tempfile.TemporaryDirectory(prefix="mf-service-package-") as directory:
        root = Path(directory)
        prefix = root / "prefix with spaces"
        subprocess.run(["make", "install", f"PREFIX={prefix}"], cwd=ROOT, check=True)
        binary = prefix / "bin/mf-service"
        unit = prefix / "lib/systemd/user/mf-service.service"
        subprocess.run([str(binary), "--help"], check=True, capture_output=True)
        assert f'ExecStart="{binary}"' in unit.read_text()
        assert "WantedBy=graphical-session.target" in unit.read_text()
        assert not (root / "graphical-session.target.wants").exists()
        runtime = root / "runtime"
        runtime.mkdir(mode=0o700)
        subprocess.run(
            [
                "systemd-analyze",
                "--user",
                "verify",
                "--man=no",
                "--generators=no",
                str(unit),
            ],
            env=dict(os.environ, XDG_RUNTIME_DIR=str(runtime)),
            check=True,
        )
        sentinel = prefix / "bin/unrelated-file"
        sentinel.write_text("keep")
        subprocess.run(["make", "uninstall", f"PREFIX={prefix}"], cwd=ROOT, check=True)
        assert sentinel.read_text() == "keep"
        assert not binary.exists() and not unit.exists()
        for invalid in ("relative/bin/mf-service", "/usr/bin/bad\nname"):
            try:
                render(invalid)
            except ValueError:
                pass
            else:
                raise AssertionError("Invalid executable path accepted")
        escaped = render("/opt/MiniFuse $test%/bin/mf-service")
        assert "$$test%%" in escaped
    print("Staged install, unit validation and removal passed; no service was started.")


if __name__ == "__main__":
    main()
