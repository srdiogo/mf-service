"""Installer verification, privilege boundaries, recovery and idempotence."""

import fcntl
import hashlib
import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "minifuse_installer", ROOT / "distribution/installer.py"
)
installer = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = installer
spec.loader.exec_module(installer)


class FakeHost(installer.Host):
    def __init__(self, root, home, env):
        super().__init__(
            root=root,
            home=home,
            env=env,
            uid=os.getuid(),
            euid=os.geteuid(),
            kernel="test-kernel",
            machine="x86_64",
        )
        self.legacy = False
        self.provider = True
        self.headers_version = "7.2.5-3"
        self.python_version = (
            f"python {sys.version_info.major}.{sys.version_info.minor}.1-1"
        )
        self.fail_package = False
        self.fail_reload = False
        self.dirty = False
        self.diagnostic_ok = True
        self.meta = {}

    def run(self, args, *, root=False, check=True):
        args = list(map(str, args))
        self.commands.append((["sudo", "--"] if root else []) + args)
        output, status = "", 0
        if args[0] == "/usr/bin/bsdtar":
            info = self.meta[Path(args[2]).name]
            output = "".join(
                f"{key} = {value}\n" for key, values in info.items() for value in values
            )
        elif args[0] == "/usr/bin/pacman":
            if args[1:3] == ["-Q", "mf-cli-platform"]:
                status = 0 if self.legacy else 1
            elif args[1] == "-Q":
                output = "linux-test 7.2.5-3\n"
            elif args[1] == "-T":
                status = 0 if self.provider else 127
            elif args[1] == "-Sp":
                output = (
                    self.python_version + "\n"
                    if args[-1] == "python"
                    else self.headers_version + "\n"
                )
            elif args[1] == "-U" and self.fail_package:
                status = 1
            elif args[1] == "-S" and "linux-test-headers" in args:
                path = self.path(f"/usr/lib/modules/{self.kernel}/build")
                path.mkdir(parents=True, exist_ok=True)
                (path / "Makefile").write_text("headers")
        elif args[0] == "/usr/bin/dkms":
            output = "minifuse_mod/test, test-kernel, x86_64: installed\n"
        elif args[0] == "/usr/bin/git":
            if "clone" in args:
                target = Path(args[-1])
                target.mkdir()
                (target / "manifest.json").write_text(
                    json.dumps({"id": installer.PLUGIN_ID})
                )
                (target / "test-commit").write_text("b" * 40)
            elif "rev-parse" in args:
                target = Path(args[args.index("-C") + 1])
                output = (target / "test-commit").read_text() + "\n"
            elif "status" in args:
                output = " M custom.qml\n" if self.dirty else ""
        elif args[0] == "hyprctl" and args[1] == "configerrors":
            if self.fail_reload and self.host_rule().exists():
                output = "injected configuration error"
        elif args[0] == "/usr/bin/minifuse-doctor":
            status = 0 if self.diagnostic_ok else 1
            output = "{}\n"
        result = subprocess.CompletedProcess(
            args, status, output, "injected failure" if status else ""
        )
        if status and check:
            raise installer.InstallError("Injected command failure")
        return result

    def host_rule(self):
        return self.config / "hypr/minifuse.lua"


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="minifuse installer ")
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.bundle = self.base / "kit"
        self.bundle.mkdir()
        self.root = self.base / "host"
        self.home = self.base / "home"
        self.home.mkdir()
        (self.root / "etc").mkdir(parents=True)
        (self.root / "etc/os-release").write_text("ID=arch\n")
        self.headers = self.root / "usr/lib/modules/test-kernel/build"
        self.headers.mkdir(parents=True)
        (self.headers / "Makefile").write_text("headers")
        runtime = self.base / "runtime"
        runtime.mkdir(mode=0o700)
        binaries = self.base / "bin"
        binaries.mkdir()
        for name in ("omarchy", "omarchy-shell"):
            path = binaries / name
            path.write_text("#!/bin/sh\nexit 0\n")
            path.chmod(0o755)
        env = {"PATH": str(binaries), "XDG_RUNTIME_DIR": str(runtime)}
        self.host = FakeHost(self.root, self.home, env)
        hypr = self.host.config / "hypr"
        hypr.mkdir(parents=True)
        (hypr / "hyprland.lua").write_text(
            '-- User config\nrequire("hypr.hyprmoncfg")\n'
        )
        self.lock = {
            "schema_version": 1,
            "api_major": 1,
            "gui": {"commit": "a" * 40},
            "plugin": {
                "id": installer.PLUGIN_ID,
                "repository": installer.PLUGIN_REPO.removesuffix(".git"),
                "commit": "b" * 40,
            },
        }
        (self.bundle / "desktop.lock.json").write_text(json.dumps(self.lock))
        backend = self.bundle / "backend"
        backend.mkdir()
        source = {
            "schema_version": 1,
            "commit": "c" * 40,
            "package_version": "0.2.0.r16.gccccccc-1",
        }
        (backend / "source-lock.json").write_text(json.dumps(source))
        (self.bundle / "gui").mkdir()
        for name in installer.PACKAGE_NAMES:
            folder = (
                "backend" if name in ("mf-service", "minifuse-driver-dkms") else "gui"
            )
            path = self.bundle / folder / f"{name}.pkg.tar.zst"
            path.write_bytes(name.encode())
            version = (
                source["package_version"]
                if folder == "backend"
                else "0.1.0.r1.gaaaaaaa-1"
            )
            self.host.meta[path.name] = {
                "pkgname": [name],
                "pkgver": [version],
                "arch": ["x86_64"],
            }
            if name == "minifuse-control-common":
                major, minor = sys.version_info[:2]
                self.host.meta[path.name]["depend"] = [
                    f"python>={major}.{minor}",
                    f"python<{major}.{minor + 1}",
                ]
        plugin = self.bundle / "plugin"
        plugin.mkdir()
        (plugin / "omarchy-minifuse.bundle").write_bytes(b"test bundle")
        self.rule = b'-- Project window policy\no.window("^io[.]github[.]srdiogo[.]MiniFuseControl$", { float = true, center = true })\n'
        with tarfile.open(
            plugin / f"omarchy-minifuse-{'b' * 40}.tar.gz", "w:gz"
        ) as stream:
            info = tarfile.TarInfo(
                f"omarchy-minifuse-{'b' * 40}/packaging/hyprland/minifuse.lua"
            )
            info.size = len(self.rule)
            stream.addfile(info, io.BytesIO(self.rule))
        for name in ("installer.py", "install.sh"):
            (self.bundle / name).write_bytes(
                (ROOT / "distribution" / name).read_bytes()
            )
        (self.bundle / "minifuse.conf").write_bytes(installer.BOOT_CONTENT)
        self.manifest()

    def manifest(self):
        path = self.bundle / "SHA256SUMS"
        path.write_text(
            "".join(
                f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(self.bundle).as_posix()}\n"
                for p in sorted(self.bundle.rglob("*"))
                if p.is_file() and p != path
            )
        )

    def instance(self, *flags):
        options = installer.arguments(["--bundle", str(self.bundle), *flags])
        return installer.Installer(self.bundle, self.host, options)

    def test_corrupt_unsafe_duplicate_and_symlink_bundle_files_are_rejected(self):
        path = self.bundle / "minifuse.conf"
        path.write_text("changed")
        with self.assertRaisesRegex(installer.InstallError, "Checksum"):
            self.instance().preflight()
        path.write_bytes(installer.BOOT_CONTENT)
        manifest = self.bundle / "SHA256SUMS"
        original = manifest.read_text()
        for extra in ("0" * 64 + "  ../outside\n", original.splitlines()[0] + "\n"):
            manifest.write_text(original + extra)
            with self.assertRaises(installer.InstallError):
                self.instance().preflight()
        manifest.write_text(original)
        path.unlink()
        path.symlink_to(self.root / "etc/os-release")
        with self.assertRaisesRegex(installer.InstallError, "symlink"):
            self.instance().preflight()
        self.assertEqual(self.host.commands, [])

    def test_dry_run_preflight_creates_nothing_and_never_requests_privilege(self):
        before = sorted(str(p) for p in self.base.rglob("*"))
        flow = self.instance("--dry-run").preflight()
        flow.plan()
        self.assertEqual(before, sorted(str(p) for p in self.base.rglob("*")))
        self.assertFalse(any(cmd[0] == "sudo" for cmd in self.host.commands))
        self.assertFalse(self.host.state.exists())

    def test_root_wrong_arch_and_foreign_runtime_are_rejected(self):
        self.host.euid = 0
        with self.assertRaisesRegex(installer.InstallError, "without sudo"):
            self.instance().preflight()
        self.host.euid = os.geteuid()
        self.host.machine = "aarch64"
        with self.assertRaisesRegex(installer.InstallError, "x86_64"):
            self.instance().preflight()
        self.host.machine = "x86_64"
        Path(self.host.env["XDG_RUNTIME_DIR"]).chmod(0o755)
        with self.assertRaisesRegex(installer.InstallError, "private"):
            self.instance().preflight()

    def test_matching_headers_are_selected_and_mismatched_repo_stops_before_changes(
        self,
    ):
        shutil.rmtree(self.headers)
        (self.headers.parent / "pkgbase").write_text("linux-test\n")
        flow = self.instance("--profile", "backend").preflight()
        self.assertEqual(flow.header, "linux-test-headers")
        self.host.headers_version = "8.0.0-1"
        with self.assertRaisesRegex(installer.InstallError, "do not match"):
            self.instance("--profile", "backend").preflight()
        self.assertFalse(any(cmd[0] == "sudo" for cmd in self.host.commands))

    def test_python_repo_and_native_minor_bounds_are_checked_before_installation(self):
        self.host.python_version = "python 9.0.1-1"
        with self.assertRaisesRegex(installer.InstallError, "Repository Python"):
            self.instance().preflight()
        self.host.python_version = (
            f"python {sys.version_info.major}.{sys.version_info.minor}.1-1"
        )
        self.host.meta["minifuse-control-common.pkg.tar.zst"]["depend"] = [
            "python>=9.0",
            "python<9.1",
        ]
        with self.assertRaisesRegex(installer.InstallError, "different Python"):
            self.instance().preflight()

    def test_legacy_driver_replacement_is_explicit_and_keep_requires_provider(self):
        self.host.legacy = True
        with self.assertRaisesRegex(installer.InstallError, "owns the existing driver"):
            self.instance().preflight()
        flow = self.instance("--keep-driver").preflight()
        self.assertNotIn("minifuse-driver-dkms", flow.selected)
        with self.assertRaisesRegex(installer.InstallError, "interactive"):
            self.instance("--replace-legacy-driver", "--yes").preflight()
        self.host.provider = False
        with self.assertRaisesRegex(installer.InstallError, "provider"):
            self.instance("--keep-driver").preflight()

    def test_full_install_keeps_session_actions_unprivileged_and_repeats_safely(self):
        flow = self.instance("--yes").preflight()
        flow.install()
        self.assertTrue(self.host.host_rule().is_file())
        text = (self.host.config / "hypr/hyprland.lua").read_text()
        self.assertLess(
            text.index(installer.REQUIRE), text.index('require("hypr.hyprmoncfg")')
        )
        self.assertTrue(flow.receipt.is_file())
        self.assertEqual(stat_mode(flow.receipt), 0o600)
        first_clone_count = sum("clone" in cmd for cmd in self.host.commands)
        self.instance("--yes").preflight().install()
        self.assertEqual(
            first_clone_count, sum("clone" in cmd for cmd in self.host.commands)
        )
        self.assertEqual(
            (self.host.config / "hypr/hyprland.lua")
            .read_text()
            .count(installer.REQUIRE),
            1,
        )
        root_commands = [cmd for cmd in self.host.commands if cmd[0] == "sudo"]
        self.assertTrue(root_commands)
        self.assertFalse(
            any(
                "--user" in cmd or "omarchy" in cmd or "hyprctl" in cmd
                for cmd in root_commands
            )
        )
        self.assertFalse(
            any(
                "rmmod" in cmd or "--remove" in cmd or "restore" in cmd
                for cmd in self.host.commands
            )
        )

    def test_concurrent_installation_is_rejected_before_mutations(self):
        flow = self.instance("--yes").preflight()
        installer.private_dir(self.host.state, self.host.uid)
        descriptor = os.open(
            self.host.state / "installer.lock", os.O_WRONLY | os.O_CREAT, 0o600
        )
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(installer.InstallError, "already running"):
                flow.install()
            self.assertFalse(any(cmd[0] == "sudo" for cmd in self.host.commands))
        finally:
            os.close(descriptor)

    def test_package_only_does_not_enable_or_reload_any_session_component(self):
        flow = self.instance("--profile", "backend", "--no-start", "--yes").preflight()
        flow.install()
        commands = self.host.commands
        self.assertFalse(
            any(
                "systemctl" in " ".join(cmd)
                or "modprobe" in " ".join(cmd)
                or "udevadm" in " ".join(cmd)
                for cmd in commands
            )
        )
        self.assertFalse(self.host.host_rule().exists())

    def test_package_failure_stops_before_session_changes(self):
        flow = self.instance("--yes").preflight()
        self.host.fail_package = True
        with self.assertRaises(installer.InstallError):
            flow.install()
        self.assertFalse(any("modprobe" in " ".join(cmd) for cmd in self.host.commands))
        self.assertFalse(flow.plugin_target.exists())

    def test_hyprland_failure_restores_exact_files_and_keeps_backup(self):
        original = (self.host.config / "hypr/hyprland.lua").read_bytes()
        original_mode = stat_mode(self.host.config / "hypr/hyprland.lua")
        flow = self.instance("--yes").preflight()
        self.host.fail_reload = True
        with self.assertRaisesRegex(installer.InstallError, "restored"):
            flow.install()
        self.assertEqual(
            (self.host.config / "hypr/hyprland.lua").read_bytes(), original
        )
        self.assertEqual(
            stat_mode(self.host.config / "hypr/hyprland.lua"), original_mode
        )
        self.assertFalse(self.host.host_rule().exists())
        self.assertEqual((flow.backups / "hyprland.lua").read_bytes(), original)

    def test_existing_plugin_requires_opt_in_and_backup_is_outside_registry(self):
        target = self.host.config / "omarchy/plugins" / installer.PLUGIN_ID
        target.mkdir(parents=True)
        (target / "manifest.json").write_text(json.dumps({"id": installer.PLUGIN_ID}))
        (target / "test-commit").write_text("d" * 40)
        (target / "custom.qml").write_text("keep my edit")
        with self.assertRaisesRegex(installer.InstallError, "customized"):
            self.instance().preflight()
        flow = self.instance("--update-plugin", "--yes").preflight()
        flow.install()
        self.assertEqual(
            (flow.backups / "plugin/custom.qml").read_text(), "keep my edit"
        )
        self.assertFalse(flow.backups.is_relative_to(target.parent))

    def test_custom_window_policy_and_boot_policy_are_not_overwritten(self):
        self.host.host_rule().write_text("-- Custom policy\no.window('custom', {})\n")
        with self.assertRaisesRegex(installer.InstallError, "Custom MiniFuse"):
            self.instance().preflight()
        self.instance("--no-floating").preflight()
        boot = self.host.path("/etc/modules-load.d/minifuse.conf")
        boot.parent.mkdir()
        boot.write_text("custom_module\n")
        with self.assertRaisesRegex(installer.InstallError, "Custom module"):
            self.instance("--no-floating").preflight()
        self.instance("--no-floating", "--no-startup").preflight()


def stat_mode(path):
    return path.stat().st_mode & 0o777


if __name__ == "__main__":
    unittest.main()
