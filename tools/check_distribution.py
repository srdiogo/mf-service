#!/usr/bin/env python3
"""Exercise staging, unit parsing and isolated process shutdown without integration."""

import os
from pathlib import Path
import subprocess
import tempfile
import time

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
        isolated = dict(
            os.environ,
            XDG_RUNTIME_DIR=str(runtime),
            XDG_STATE_HOME=str(root / "state"),
            XDG_DATA_HOME=str(root / "data"),
        )
        process = subprocess.Popen(
            [str(binary)],
            cwd=root,
            env=isolated,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        endpoint = runtime / "mf-service/control.sock"
        try:
            deadline = time.monotonic() + 2
            while (
                not endpoint.exists()
                and process.poll() is None
                and time.monotonic() < deadline
            ):
                time.sleep(0.01)
            assert endpoint.exists(), (
                "Isolated package instance did not create its socket"
            )
            process.terminate()
            assert process.wait(timeout=5) == 0
            assert not endpoint.exists(), "SIGTERM left a stale socket"
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=2)
            process.communicate()
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
    print(
        "Staging, unit parsing, isolated SIGTERM cleanup and removal passed; no startup was enabled."
    )


if __name__ == "__main__":
    main()
