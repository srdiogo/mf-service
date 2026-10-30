"""Release-source integrity and read-only diagnostic boundaries."""

import hashlib
import io
import json
import os
import socket
import subprocess
import sys
import tarfile
import tempfile
import threading
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from minifuse_doctor import diagnose, probe
from prepare_arch_package import export

PROJECT = Path(__file__).resolve().parents[2]


class SourceKitTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.repo = self.base / "repo"
        self.repo.mkdir()
        self.git("init", "--quiet")
        self.git("config", "user.name", "Package Test")
        self.git("config", "user.email", "package-test@example.invalid")
        (self.repo / "crates/mf-service").mkdir(parents=True)
        (self.repo / "crates/mf-service/Cargo.toml").write_text(
            '[package]\nname="mf-service"\nversion="0.2.0"\n'
        )
        (self.repo / "packaging/arch").mkdir(parents=True)
        (self.repo / "packaging/arch/PKGBUILD.in").write_bytes(
            (PROJECT / "packaging/arch/PKGBUILD.in").read_bytes()
        )
        (self.repo / "payload").write_text("reviewed source")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "Add package fixture")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repo), *args])

    def test_export_is_reproducible_and_binds_recipe_to_committed_source(self):
        first = self.base / "first"
        second = self.base / "second"
        profile = export(first, root=self.repo)
        self.assertEqual(profile, export(second, root=self.repo))
        for path in first.iterdir():
            self.assertEqual(path.read_bytes(), (second / path.name).read_bytes())
        for name, expected in profile["files"].items():
            self.assertEqual(
                hashlib.sha256((first / name).read_bytes()).hexdigest(), expected
            )
        archive = next(first.glob("*.tar.gz"))
        with tarfile.open(fileobj=io.BytesIO(archive.read_bytes())) as stream:
            content = stream.extractfile(
                f"mf-service-{profile['commit']}/payload"
            ).read()
        self.assertEqual(content, b"reviewed source")
        recipe = (first / "PKGBUILD").read_text()
        self.assertIn(profile["commit"], recipe)
        self.assertNotIn("mf-cli", recipe.replace("mf-cli-platform", ""))
        self.assertNotIn("@VERSION@", recipe)
        subprocess.run(["bash", "-n", str(first / "PKGBUILD")], check=True)

    def test_dirty_or_untracked_content_cannot_be_silently_omitted(self):
        (self.repo / "private-file").write_text("not reviewed")
        with self.assertRaisesRegex(ValueError, "Commit"):
            export(self.base / "out", root=self.repo)
        (self.repo / "private-file").unlink()
        (self.repo / "payload").write_text("uncommitted change")
        with self.assertRaisesRegex(ValueError, "Commit"):
            export(self.base / "out", root=self.repo)

    def test_existing_output_is_not_overwritten(self):
        destination = self.base / "out"
        destination.mkdir()
        (destination / "keep").write_text("keep")
        with self.assertRaisesRegex(ValueError, "empty"):
            export(destination, root=self.repo)
        self.assertEqual((destination / "keep").read_text(), "keep")


class DoctorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="mf-doctor-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.root.chmod(0o700)

    def test_missing_dependencies_report_errors_without_creating_files(self):
        before = list(self.root.rglob("*"))
        report = diagnose(self.root, {"PATH": ""}, kernel="test", no_service=True)
        self.assertFalse(report["ok"])
        self.assertEqual(list(self.root.rglob("*")), before)
        checks = {item["check"]: item["status"] for item in report["checks"]}
        self.assertEqual(checks["driver"], "error")
        self.assertEqual(checks["gui"], "optional")
        self.assertEqual(checks["kernel_headers"], "optional")

    def test_socket_probe_uses_only_two_reads_and_redacts_identity(self):
        directory = self.root / "mf-service"
        directory.mkdir(mode=0o700)
        endpoint = directory / "control.sock"
        requests = []
        errors = []
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
            server.bind(str(endpoint))
            endpoint.chmod(0o600)
            server.listen(1)
            server.settimeout(5)

            def serve():
                try:
                    connection, _ = server.accept()
                    with connection, connection.makefile("rb") as stream:
                        for result in (
                            {"selected_api": 1, "client_token": "private"},
                            {
                                "devices": [
                                    {
                                        "device_id": "private",
                                        "serial": "private",
                                        "product_id": 0xAF70,
                                        "connection": "ready",
                                        "capabilities": {
                                            name: True
                                            for name in (
                                                "mixer",
                                                "switch_readback",
                                                "input_switches",
                                                "signal_meters",
                                            )
                                        },
                                    }
                                ]
                            },
                        ):
                            request = json.loads(stream.readline())
                            requests.append(request)
                            connection.sendall(
                                json.dumps(
                                    {"api": 1, "id": request["id"], "result": result}
                                ).encode()
                                + b"\n"
                            )
                except Exception as error:
                    errors.append(error)

            thread = threading.Thread(target=serve)
            thread.start()
            try:
                report = probe(self.root, os.getuid())
            finally:
                thread.join(timeout=6)
            self.assertFalse(thread.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(
            [item["method"] for item in requests], ["hello", "list_devices"]
        )
        self.assertEqual(
            report,
            {
                "api_major": 1,
                "device_count": 1,
                "mf4_ready": True,
                "verified_features": True,
            },
        )
        self.assertNotIn("private", json.dumps(report))

    def test_untrusted_runtime_and_symlinks_are_rejected_before_connect(self):
        self.root.chmod(0o755)
        with self.assertRaisesRegex(ValueError, "private"):
            probe(self.root, os.getuid())
        self.root.chmod(0o700)
        link = self.root / "runtime-link"
        link.symlink_to(self.root, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "private"):
            probe(link, os.getuid())


if __name__ == "__main__":
    unittest.main()
