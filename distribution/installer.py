#!/usr/bin/env python3
"""Install the verified MiniFuse kit on Arch/Omarchy from a normal user session."""

import argparse
import fcntl
import hashlib
import json
import os
import platform
import re
import shlex
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

PLUGIN_ID = "srdiogo.minifuse"
PLUGIN_REPO = "https://github.com/srdiogo/omarchy-minifuse.git"
SERVICE = "mf-service.service"
BOOT_CONTENT = b"minifuse_mod\n"
REQUIRE = 'require("hypr.minifuse")'
PACKAGE_NAMES = {
    "mf-service",
    "minifuse-driver-dkms",
    "minifuse-control-common",
    "minifuse-control",
    "minifuse-desktop-agent",
}


class InstallError(Exception):
    pass


def regular(path):
    if path.is_symlink() or not path.is_file():
        raise InstallError(f"Expected a regular file: {path}")
    return path


def load_json(path):
    regular(path)
    if path.stat().st_size > 1024 * 1024:
        raise InstallError(f"Metadata is too large: {path.name}")
    data = json.loads(path.read_text())
    if not isinstance(data, dict):
        raise InstallError(f"Expected object metadata: {path.name}")
    return data


def digest(path):
    with regular(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_bundle(root, require_installer=True):
    root = root.resolve()
    manifest = regular(root / "SHA256SUMS")
    entries = {}
    for line in manifest.read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([^\x00-\x1f\\]+)", line)
        if not match:
            raise InstallError("Malformed SHA256SUMS entry")
        expected, name = match.groups()
        path = PurePosixPath(name)
        if path.is_absolute() or ".." in path.parts or name in entries:
            raise InstallError("Unsafe or duplicate bundle path")
        target = root.joinpath(*path.parts)
        current = root
        for part in path.parts:
            current = current / part
            if current.is_symlink():
                raise InstallError(f"Bundle symlinks are not accepted: {name}")
        if digest(target) != expected:
            raise InstallError(f"Checksum mismatch: {name}")
        entries[name] = expected
    required_files = ["desktop.lock.json", "backend/source-lock.json"]
    if require_installer:
        required_files += ["installer.py", "install.sh", "minifuse.conf"]
    for required in required_files:
        if required not in entries:
            raise InstallError(f"Unverified required file: {required}")
    lock = load_json(root / "desktop.lock.json")
    source = load_json(root / "backend/source-lock.json")
    if (
        not isinstance(lock.get("gui"), dict)
        or not isinstance(lock.get("plugin"), dict)
        or not isinstance(source.get("package_version"), str)
    ):
        raise InstallError("Incomplete kit metadata")
    if (
        lock.get("schema_version") != 1
        or lock.get("api_major") != 1
        or source.get("schema_version") != 1
    ):
        raise InstallError("Unsupported kit metadata")
    if lock.get("plugin", {}).get("id") != PLUGIN_ID or lock["plugin"].get(
        "repository"
    ) != PLUGIN_REPO.removesuffix(".git"):
        raise InstallError("Unexpected plugin identity")
    for commit in (
        lock["plugin"].get("commit", ""),
        lock.get("gui", {}).get("commit", ""),
        source.get("commit", ""),
    ):
        if not re.fullmatch(r"[0-9a-f]{40}", commit):
            raise InstallError("The kit must pin full source revisions")
    if not entries:
        raise InstallError("Empty installation kit")
    return entries, lock, source


@dataclass
class Host:
    root: Path = Path("/")
    home: Path = field(default_factory=Path.home)
    env: dict = field(default_factory=lambda: dict(os.environ))
    uid: int = field(default_factory=os.getuid)
    euid: int = field(default_factory=os.geteuid)
    kernel: str = field(default_factory=platform.release)
    machine: str = field(default_factory=platform.machine)
    commands: list = field(default_factory=list)

    def path(self, path):
        return self.root / path.lstrip("/")

    def run(self, args, *, root=False, check=True):
        command = (["/usr/bin/sudo", "--"] if root else []) + list(map(str, args))
        self.commands.append(command)
        result = subprocess.run(
            command, text=True, capture_output=not root, env=dict(self.env, LC_ALL="C")
        )
        if check and result.returncode:
            raise InstallError(
                f"Command failed: {shlex.join(command)}\n{(result.stderr or result.stdout or f'exit {result.returncode}').strip()}"
            )
        return result

    @property
    def config(self):
        path = Path(self.env.get("XDG_CONFIG_HOME") or self.home / ".config")
        if not path.is_absolute():
            raise InstallError("XDG_CONFIG_HOME must be absolute")
        return path

    @property
    def state(self):
        path = Path(self.env.get("XDG_STATE_HOME") or self.home / ".local/state")
        if not path.is_absolute():
            raise InstallError("XDG_STATE_HOME must be absolute")
        return path / "minifuse-installer"


def package_info(host, path):
    raw = host.run(["/usr/bin/bsdtar", "-xOf", path, ".PKGINFO"]).stdout
    fields = {}
    for line in raw.splitlines():
        if " = " in line:
            key, value = line.split(" = ", 1)
            fields.setdefault(key, []).append(value)
    for key in ("pkgname", "pkgver", "arch"):
        if len(fields.get(key, [])) != 1:
            raise InstallError(f"Invalid package metadata: {path.name}")
    if fields["pkgname"][0] not in PACKAGE_NAMES:
        raise InstallError(f"Unexpected package: {path.name}")
    return fields


def owned_file(path, uid):
    regular(path)
    info = path.stat()
    if info.st_uid != uid or info.st_mode & 0o022:
        raise InstallError(
            f"File must be owned by the current user and not writable by others: {path}"
        )


def private_dir(path, uid):
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = path.lstat()
    if (
        not stat.S_ISDIR(info.st_mode)
        or info.st_uid != uid
        or stat.S_IMODE(info.st_mode) != 0o700
    ):
        raise InstallError(f"Expected an owned mode-0700 directory: {path}")


def atomic_write(path, content, mode=0o600):
    descriptor, temporary = tempfile.mkstemp(prefix=".minifuse-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


class Installer:
    def __init__(self, bundle, host, options):
        self.bundle = bundle.resolve()
        self.host = host
        self.options = options
        self.entries = {}
        self.packages = {}
        self.dependencies = ["base-devel", "dkms", "python"]
        self.completed = []
        self.receipt = None
        self.backups = None

    def checked(self, relative):
        if relative not in self.entries:
            raise InstallError(f"Unverified bundle file: {relative}")
        return self.bundle / relative

    def preflight(self):
        h, o = self.host, self.options
        if h.uid == 0 or h.euid == 0:
            raise InstallError("Run as the graphical-session user, without sudo")
        if h.machine != "x86_64" or not re.search(
            r"^(ID=arch|ID_LIKE=.*\barch\b)",
            h.path("/etc/os-release").read_text(),
            re.M,
        ):
            raise InstallError("Native packages require Arch Linux x86_64")
        self.entries, self.lock, self.source = verify_bundle(self.bundle)
        self.gui = o.profile in ("full", "gui") or (o.profile == "auto")
        self.plugin = o.profile in ("full", "omarchy") or (
            o.profile == "auto" and shutil.which("omarchy", path=h.env.get("PATH"))
        )
        self.floating = (
            self.gui
            and not o.no_floating
            and bool(shutil.which("omarchy", path=h.env.get("PATH")))
        )
        if self.plugin:
            if not shutil.which("omarchy", path=h.env.get("PATH")) or not shutil.which(
                "omarchy-shell", path=h.env.get("PATH")
            ):
                raise InstallError(
                    "Omarchy is missing; use --profile gui or --profile backend"
                )
            self.dependencies.append("git")
            self.checked("plugin/omarchy-minifuse.bundle")
        if self.gui:
            self.dependencies += ["pyside6", "qt6-wayland"]
        if self.gui or o.desktop_agent:
            planned_python = h.run(
                [
                    "/usr/bin/pacman",
                    "-Sp",
                    "--nodeps",
                    "--nodeps",
                    "--print-format",
                    "%n %v",
                    "--",
                    "python",
                ]
            ).stdout.strip()
            match = re.fullmatch(r"python (\d+)\.(\d+)\..+", planned_python)
            if not match or tuple(map(int, match.groups())) != sys.version_info[:2]:
                raise InstallError(
                    "Repository Python differs from this kit's interpreter minor version; rebuild the GUI packages before installing"
                )
        if (
            not o.no_startup
            and self.checked("minifuse.conf").read_bytes() != BOOT_CONTENT
        ):
            raise InstallError("Unexpected module-loading policy")
        proc_version = h.path("/proc/version")
        if proc_version.is_file() and "clang" in proc_version.read_text().lower():
            self.dependencies += ["clang", "llvm", "lld"]
        self.header = self.find_headers()
        for folder in ("backend", "gui"):
            for name in sorted(self.entries):
                if name.startswith(folder + "/") and name.endswith(".pkg.tar.zst"):
                    path = self.checked(name)
                    info = package_info(h, path)
                    package = info["pkgname"][0]
                    if package in self.packages:
                        raise InstallError("Duplicate package in kit")
                    if info["arch"][0] not in ("any", "x86_64"):
                        raise InstallError("Incompatible native package architecture")
                    self.packages[package] = (path, info)
        selected = ["minifuse-driver-dkms", "mf-service"]
        if self.gui:
            selected += ["minifuse-control-common", "minifuse-control"]
        if o.desktop_agent:
            selected += ["minifuse-control-common", "minifuse-desktop-agent"]
        for name in dict.fromkeys(selected):
            if name not in self.packages:
                raise InstallError(f"Required package is missing: {name}")
        for name in ("minifuse-driver-dkms", "mf-service"):
            if self.packages[name][1]["pkgver"][0] != self.source["package_version"]:
                raise InstallError("Backend package does not match the source lock")
        if self.gui or o.desktop_agent:
            dependencies = self.packages["minifuse-control-common"][1].get("depend", [])
            minor = sys.version_info[:2]
            if (
                f"python>={minor[0]}.{minor[1]}" not in dependencies
                or f"python<{minor[0]}.{minor[1] + 1}" not in dependencies
            ):
                raise InstallError(
                    "GUI packages target a different Python minor version; rebuild them for this machine"
                )
        legacy = (
            h.run(["/usr/bin/pacman", "-Q", "mf-cli-platform"], check=False).returncode
            == 0
        )
        if legacy and not o.keep_driver and not o.replace_legacy_driver:
            raise InstallError(
                "mf-cli-platform owns the existing driver. Choose --keep-driver or explicitly --replace-legacy-driver (also removes its CLI)"
            )
        if legacy and o.replace_legacy_driver and o.yes:
            raise InstallError(
                "Legacy driver replacement requires pacman's interactive conflict review; omit --yes"
            )
        if o.keep_driver:
            if h.run(
                ["/usr/bin/pacman", "-T", "minifuse-transport=1"], check=False
            ).returncode:
                raise InstallError(
                    "--keep-driver requires an installed compatible transport provider"
                )
            selected.remove("minifuse-driver-dkms")
        self.selected = list(dict.fromkeys(selected))
        if not o.no_start:
            runtime = Path(h.env.get("XDG_RUNTIME_DIR", ""))
            if (
                not runtime.is_absolute()
                or runtime.is_symlink()
                or not runtime.is_dir()
                or runtime.stat().st_uid != h.uid
                or stat.S_IMODE(runtime.stat().st_mode) != 0o700
            ):
                raise InstallError(
                    "Run inside the owning graphical session with a private XDG_RUNTIME_DIR"
                )
            h.run(
                [
                    "/usr/bin/systemctl",
                    "--user",
                    "show",
                    "--property=Version",
                    "--value",
                ]
            )
        boot = h.path("/etc/modules-load.d/minifuse.conf")
        if (
            not o.no_startup
            and (boot.exists() or boot.is_symlink())
            and regular(boot).read_bytes().strip() != BOOT_CONTENT.strip()
        ):
            raise InstallError(
                "Custom module boot policy exists; use --no-startup to preserve it"
            )
        self.plugin_target = h.config / "omarchy/plugins" / PLUGIN_ID
        if self.plugin and (
            self.plugin_target.exists() or self.plugin_target.is_symlink()
        ):
            if (
                self.plugin_target.is_symlink()
                or self.plugin_target.stat().st_uid != h.uid
            ):
                raise InstallError(
                    "Existing plugin must be an owned directory, not a symlink"
                )
            installed = load_json(self.plugin_target / "manifest.json")
            if installed.get("id") != PLUGIN_ID:
                raise InstallError("Existing plugin path belongs to another project")
            current = h.run(
                ["/usr/bin/git", "-C", self.plugin_target, "rev-parse", "HEAD"]
            ).stdout.strip()
            dirty = h.run(
                ["/usr/bin/git", "-C", self.plugin_target, "status", "--porcelain"]
            ).stdout.strip()
            if (
                current != self.lock["plugin"]["commit"] or dirty
            ) and not o.update_plugin:
                raise InstallError(
                    "A different/customized plugin exists; use --update-plugin to back it up and replace it"
                )
        if self.floating:
            self.hypr_main = h.config / "hypr/hyprland.lua"
            self.hypr_rule = h.config / "hypr/minifuse.lua"
            owned_file(self.hypr_main, h.uid)
            if self.hypr_rule.exists() or self.hypr_rule.is_symlink():
                owned_file(self.hypr_rule, h.uid)
                if self.normalized_rule(
                    self.hypr_rule.read_text()
                ) != self.normalized_rule(self.floating_rule().decode()):
                    raise InstallError(
                        "Custom MiniFuse window policy exists; use --no-floating to preserve it"
                    )
            errors = h.run(["hyprctl", "configerrors"]).stdout.strip()
            if errors:
                raise InstallError(
                    "Fix existing Hyprland configuration errors before enabling floating-window setup, or use --no-floating"
                )
        return self

    def find_headers(self):
        h, o = self.host, self.options
        build = h.path(f"/usr/lib/modules/{h.kernel}/build")
        if (build / "Makefile").is_file():
            config = build / ".config"
            if config.is_file() and "CONFIG_CC_IS_CLANG=y" in config.read_text():
                self.dependencies += ["clang", "llvm", "lld"]
            return None
        pkgbase_file = h.path(f"/usr/lib/modules/{h.kernel}/pkgbase")
        pkgbase = pkgbase_file.read_text().strip() if pkgbase_file.is_file() else ""
        package = o.headers or (pkgbase + "-headers" if pkgbase else "")
        if not re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9+_.-]*", package) or (
            pkgbase and not re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9+_.-]*", pkgbase)
        ):
            raise InstallError(
                "Cannot identify matching kernel headers; provide --headers PACKAGE"
            )
        if not pkgbase:
            raise InstallError(
                "The running kernel has no package identity; install its matching headers manually"
            )
        installed = h.run(["/usr/bin/pacman", "-Q", pkgbase]).stdout.strip().split()
        repository = (
            h.run(
                [
                    "/usr/bin/pacman",
                    "-Sp",
                    "--nodeps",
                    "--nodeps",
                    "--print-format",
                    "%v",
                    "--",
                    package,
                ]
            )
            .stdout.strip()
            .splitlines()
        )
        if len(installed) != 2 or repository != [installed[1]]:
            raise InstallError(
                "Repository headers do not match the installed kernel. Update/reboot separately or install matching headers; no system upgrade will run automatically"
            )
        return package

    @staticmethod
    def normalized_rule(text):
        return "\n".join(
            line.strip()
            for line in text.splitlines()
            if line.strip() and not line.lstrip().startswith("--")
        )

    def floating_rule(self):
        revision = self.lock["plugin"]["commit"]
        path = self.checked(f"plugin/omarchy-minifuse-{revision}.tar.gz")
        with tarfile.open(path) as stream:
            member = stream.getmember(
                f"omarchy-minifuse-{revision}/packaging/hyprland/minifuse.lua"
            )
            if not member.isfile() or member.size > 4096:
                raise InstallError("Invalid packaged window policy")
            return stream.extractfile(member).read()

    def plan(self):
        startup = (
            "enabled at login/boot"
            if not self.options.no_startup
            else "unchanged; current-session start only"
        )
        print("MiniFuse installation plan")
        print("  Kit source: " + self.source["commit"])
        print(
            "  System dependencies: "
            + ", ".join(self.dependencies + ([self.header] if self.header else []))
        )
        print("  Native packages: " + ", ".join(self.selected))
        print(
            "  Startup: "
            + ("unchanged; packages only" if self.options.no_start else startup)
        )
        print(
            "  Omarchy plugin: "
            + (
                "pinned source, enabled in the right bar section"
                if self.plugin and not self.options.no_start
                else "pinned source, not enabled"
                if self.plugin
                else "skip"
            )
        )
        print(
            "  GUI floating rule: "
            + ("install with backup and validation" if self.floating else "skip")
        )
        print(
            "  Desktop agent: "
            + (
                "package only; activation remains disabled"
                if self.options.desktop_agent
                else "skip (logo events unavailable)"
            )
        )
        print("  Device settings and user presets are preserved.")

    def mark(self, stage):
        self.completed.append(stage)
        if self.receipt:
            atomic_write(
                self.receipt,
                (
                    json.dumps(
                        {
                            "schema_version": 1,
                            "kit_commit": self.source["commit"],
                            "completed": self.completed,
                        },
                        indent=2,
                    )
                    + "\n"
                ).encode(),
            )

    def install(self):
        private_dir(self.host.state, self.host.uid)
        descriptor = os.open(
            self.host.state / "installer.lock",
            os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW,
            0o600,
        )
        try:
            info = os.fstat(descriptor)
            if (
                not stat.S_ISREG(info.st_mode)
                or info.st_uid != self.host.uid
                or stat.S_IMODE(info.st_mode) != 0o600
            ):
                raise InstallError("Invalid installer lock ownership or permissions")
            try:
                fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise InstallError(
                    "Another MiniFuse installer is already running"
                ) from None
            self._install()
        finally:
            os.close(descriptor)

    def _install(self):
        h, o = self.host, self.options
        private_dir(h.state, h.uid)
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
        self.receipt = h.state / f"install-{stamp}.json"
        self.backups = h.state / f"backups-{stamp}"
        private_dir(self.backups, h.uid)
        flags = ["--noconfirm"] if o.yes else []
        dependencies = list(
            dict.fromkeys(self.dependencies + ([self.header] if self.header else []))
        )
        print("Installing distribution prerequisites…", flush=True)
        h.run(
            ["/usr/bin/pacman", "-S", "--needed", *flags, "--", *dependencies],
            root=True,
        )
        self.mark("prerequisites")
        if not h.path(f"/usr/lib/modules/{h.kernel}/build/Makefile").is_file():
            raise InstallError(
                "Matching running-kernel headers are still missing. Stop here and resolve the kernel/header version; backend packages were not installed"
            )
        print("Installing native MiniFuse packages…", flush=True)
        h.run(
            [
                "/usr/bin/pacman",
                "-U",
                "--needed",
                *flags,
                "--",
                *[self.packages[name][0] for name in self.selected],
            ],
            root=True,
        )
        self.mark("packages")
        if not o.no_start:
            status_args = [
                "/usr/bin/dkms",
                "status",
                "-m",
                "minifuse_mod",
                "-k",
                h.kernel,
            ]
            version = self.source["package_version"].rsplit("-", 1)[0]
            if not o.keep_driver:
                status_args += ["-v", version]
            if ": installed" not in h.run(status_args).stdout:
                if o.keep_driver:
                    raise InstallError(
                        "The retained driver is not installed for the running kernel; repair its DKMS installation first"
                    )
                h.run(
                    [
                        "/usr/bin/dkms",
                        "install",
                        "-m",
                        "minifuse_mod",
                        "-v",
                        version,
                        "-k",
                        h.kernel,
                    ],
                    root=True,
                )
            h.run(["/usr/bin/udevadm", "control", "--reload-rules"], root=True)
            h.run(["/usr/bin/modprobe", "minifuse_mod"], root=True)
            h.run(
                [
                    "/usr/bin/udevadm",
                    "trigger",
                    "--subsystem-match=misc",
                    "--sysname-match=minifuse_cmd",
                ],
                root=True,
            )
            h.run(["/usr/bin/udevadm", "settle"], root=True)
            if not o.no_startup:
                h.run(
                    [
                        "/usr/bin/install",
                        "-Dm644",
                        self.checked("minifuse.conf"),
                        "/etc/modules-load.d/minifuse.conf",
                    ],
                    root=True,
                )
            h.run(["/usr/bin/systemctl", "--user", "daemon-reload"])
            h.run(
                [
                    "/usr/bin/systemctl",
                    "--user",
                    *(["enable", "--now"] if not o.no_startup else ["start"]),
                    SERVICE,
                ]
            )
            self.mark("driver-and-session")
        if self.plugin:
            self.install_plugin()
        if self.floating:
            self.install_floating()
        if not o.no_start:
            report = h.run(["/usr/bin/minifuse-doctor", "--json"], check=False)
            self.mark(
                "diagnostic-passed"
                if report.returncode == 0
                else "diagnostic-needs-attention"
            )
            if report.returncode:
                print(
                    "Installation completed; diagnostic needs attention (device absent, access, or older loaded driver)."
                )
                print(report.stdout.strip() or report.stderr.strip())
                print(
                    "An already loaded older module is never unloaded automatically. A suitable reboot may be needed."
                )
        self.mark("complete")
        print(f"Installation complete. Local record and backups: {h.state}")

    def install_plugin(self):
        h, o = self.host, self.options
        target = self.plugin_target
        target.parent.mkdir(parents=True, exist_ok=True)
        same = False
        if target.exists():
            same = (
                h.run(
                    ["/usr/bin/git", "-C", target, "rev-parse", "HEAD"]
                ).stdout.strip()
                == self.lock["plugin"]["commit"]
                and not h.run(
                    ["/usr/bin/git", "-C", target, "status", "--porcelain"]
                ).stdout.strip()
            )
        if not same:
            with tempfile.TemporaryDirectory(
                prefix="plugin-", dir=h.state
            ) as directory:
                staging = Path(directory) / "checkout"
                h.run(
                    [
                        "/usr/bin/git",
                        "-c",
                        "core.hooksPath=/dev/null",
                        "clone",
                        "--no-checkout",
                        "--",
                        self.checked("plugin/omarchy-minifuse.bundle"),
                        staging,
                    ]
                )
                h.run(
                    [
                        "/usr/bin/git",
                        "-c",
                        "core.hooksPath=/dev/null",
                        "-C",
                        staging,
                        "checkout",
                        "--detach",
                        self.lock["plugin"]["commit"],
                    ]
                )
                if load_json(staging / "manifest.json").get("id") != PLUGIN_ID:
                    raise InstallError("Bundled checkout has the wrong plugin identity")
                h.run(
                    [
                        "/usr/bin/git",
                        "-C",
                        staging,
                        "remote",
                        "set-url",
                        "origin",
                        PLUGIN_REPO,
                    ]
                )
                h.run(["omarchy", "plugin", "validate", staging])
                if target.exists():
                    if not o.update_plugin:
                        raise InstallError(
                            "Plugin changed during installation; refusing to replace it"
                        )
                    shutil.move(str(target), str(self.backups / "plugin"))
                try:
                    shutil.move(str(staging), str(target))
                except OSError:
                    if (self.backups / "plugin").exists() and not target.exists():
                        shutil.move(str(self.backups / "plugin"), str(target))
                    raise
        self.mark("plugin-files")
        if not o.no_start:
            h.run(["omarchy-shell", "shell", "rescanPlugins"])
            for attempt in range(20):
                result = h.run(
                    ["omarchy", "plugin", "enable", PLUGIN_ID, "--section", "right"],
                    check=False,
                )
                if result.returncode == 0:
                    break
                if "not known" not in (
                    result.stderr + result.stdout
                ) and "unknown" not in (result.stderr + result.stdout):
                    raise InstallError(
                        (
                            result.stderr
                            or result.stdout
                            or f"exit {result.returncode}"
                        ).strip()
                    )
                time.sleep(0.1)
            else:
                raise InstallError(
                    "Plugin was installed but the host has not discovered it; rerun after checking Omarchy"
                )
        self.mark("plugin")

    def install_floating(self):
        h = self.host
        main, rule = self.hypr_main, self.hypr_rule
        owned_file(main, h.uid)
        if rule.exists() or rule.is_symlink():
            owned_file(rule, h.uid)
            if self.normalized_rule(rule.read_text()) != self.normalized_rule(
                self.floating_rule().decode()
            ):
                raise InstallError(
                    "Window policy changed during installation; preserving the custom file"
                )
        original = main.read_bytes()
        original_mode = stat.S_IMODE(main.stat().st_mode)
        old_rule = rule.read_bytes() if rule.exists() else None
        text = original.decode()
        if not re.search(
            r'^\s*require\(["\']hypr\.minifuse["\']\)\s*(?:--.*)?$', text, re.M
        ):
            anchor = re.search(
                r'^\s*require\(["\']hypr\.hyprmoncfg["\']\).*$', text, re.M
            )
            position = anchor.start() if anchor else len(text)
            text = text[:position].rstrip() + "\n" + REQUIRE + "\n" + text[position:]
        shutil.copy2(main, self.backups / "hyprland.lua")
        if old_rule is not None:
            shutil.copy2(rule, self.backups / "minifuse.lua")
        try:
            if old_rule is None:
                atomic_write(rule, self.floating_rule(), 0o644)
            if text.encode() != original:
                atomic_write(main, text.encode(), stat.S_IMODE(main.stat().st_mode))
            h.run(["hyprctl", "reload"])
            if h.run(["hyprctl", "configerrors"]).stdout.strip():
                raise InstallError("Hyprland rejected the floating-window policy")
        except (OSError, InstallError):
            atomic_write(main, original, original_mode)
            if old_rule is None:
                rule.unlink(missing_ok=True)
            else:
                atomic_write(rule, old_rule)
            h.run(["hyprctl", "reload"], check=False)
            raise InstallError(
                "Floating-window setup failed; original files were restored. Backups are retained"
            ) from None
        self.mark("floating-window")


def arguments(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument(
        "--profile",
        choices=("auto", "full", "backend", "gui", "omarchy"),
        default="auto",
        help="auto installs GUI and also the plugin when Omarchy is present",
    )
    parser.add_argument(
        "--dry-run",
        "--check",
        action="store_true",
        help="Verify and display the complete plan without changes",
    )
    parser.add_argument(
        "--yes",
        action="store_true",
        help="Accept the displayed plan and pacman prompts (sudo may still authenticate)",
    )
    parser.add_argument("--headers", help="Explicit matching kernel-header package")
    parser.add_argument(
        "--no-startup",
        action="store_true",
        help="Preserve boot/login startup; start only this session",
    )
    parser.add_argument(
        "--no-start",
        action="store_true",
        help="Install files only; do not load/start/enable components",
    )
    parser.add_argument(
        "--no-floating", action="store_true", help="Preserve compositor configuration"
    )
    parser.add_argument(
        "--keep-driver",
        action="store_true",
        help="Keep the compatible installed transport provider",
    )
    parser.add_argument(
        "--replace-legacy-driver",
        action="store_true",
        help="Allow pacman's interactive replacement of mf-cli-platform",
    )
    parser.add_argument(
        "--update-plugin",
        action="store_true",
        help="Back up and replace an existing different/customized plugin",
    )
    parser.add_argument(
        "--desktop-agent",
        action="store_true",
        help="Install the optional agent package without enabling it",
    )
    options = parser.parse_args(argv)
    if options.keep_driver and options.replace_legacy_driver:
        parser.error("--keep-driver and --replace-legacy-driver are mutually exclusive")
    if options.no_start:
        options.no_startup = True
        options.no_floating = True
    return options


def main(argv=None):
    if sys.version_info < (3, 12):
        print("Python 3.12+ is required", file=sys.stderr)
        return 1
    options = arguments(argv)
    installer = None
    try:
        installer = Installer(options.bundle, Host(), options).preflight()
        installer.plan()
        if options.dry_run:
            print(
                "Verification complete; no packages, files, services or settings changed."
            )
            return 0
        if not options.yes and input(
            "Apply this installation plan? [y/N] "
        ).strip().lower() not in ("y", "yes"):
            print("Cancelled; no changes made.")
            return 0
        installer.install()
        return 0
    except (InstallError, OSError, ValueError, EOFError, KeyboardInterrupt) as error:
        print(f"Installation stopped: {error}", file=sys.stderr)
        if installer and installer.completed:
            print(
                "Completed stages: " + ", ".join(installer.completed), file=sys.stderr
            )
            print(
                f"Review the record/backups in {installer.host.state}; rerun to resume. User presets were not removed.",
                file=sys.stderr,
            )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
