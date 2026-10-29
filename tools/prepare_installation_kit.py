#!/usr/bin/env python3
"""Add the standalone installer to an already verified native-package kit."""

import argparse
import gzip
import hashlib
import importlib.util
import json
import shutil
import subprocess
import sys
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "kit_installer", ROOT / "distribution/installer.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


def prepare(base, output):
    base, output = base.resolve(), output.resolve()
    if output == base or output.is_relative_to(base):
        raise ValueError("Output must be separate from the existing kit")
    archive = output.with_name(output.name + ".tar.gz")
    if archive.exists():
        raise ValueError("Archive already exists")
    if output.exists():
        raise ValueError("Output already exists; choose a new kit directory")
    if subprocess.check_output(
        ["git", "-C", str(ROOT), "status", "--porcelain"]
    ).strip():
        raise ValueError("Commit the installer changes before preparing a release kit")
    revision = subprocess.check_output(
        ["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True
    ).strip()
    entries, _, _ = module.verify_bundle(base, require_installer=False)
    output.mkdir(parents=True)
    for name in entries:
        if name == "README.md":
            continue
        destination = output / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(base / name, destination)
    for name in ("install.sh", "installer.py"):
        shutil.copy2(ROOT / "distribution" / name, output / name)
    shutil.copy2(ROOT / "distribution/INSTALLER.md", output / "README.md")
    shutil.copy2(ROOT / "packaging/minifuse.conf.example", output / "minifuse.conf")
    shutil.copy2(ROOT / "LICENSE", output / "INSTALLER-LICENSE")
    record = module.load_json(output / "BUILD-RECORD.json")
    record["installer"] = {
        "repository": "srdiogo/mf-service",
        "revision": revision,
        "entrypoint": "install.sh",
    }
    record["checks"].append(
        "Standalone installer tests cover preflight, privilege boundaries, retries, backups and rollback using isolated fake hosts"
    )
    (output / "BUILD-RECORD.json").write_text(json.dumps(record, indent=2) + "\n")
    manifest = output / "SHA256SUMS"
    manifest.write_text(
        "".join(
            f"{module.digest(path)}  {path.relative_to(output).as_posix()}\n"
            for path in sorted(output.rglob("*"))
            if path.is_file() and path != manifest
        )
    )
    module.verify_bundle(output)
    archive = output.with_name(output.name + ".tar.gz")
    if archive.exists():
        raise ValueError("Archive already exists")
    with (
        archive.open("wb") as raw,
        gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed,
        tarfile.open(fileobj=compressed, mode="w") as stream,
    ):
        for path in sorted(output.rglob("*")):
            info = stream.gettarinfo(
                str(path),
                arcname=output.name + "/" + path.relative_to(output).as_posix(),
            )
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mtime = 0
            if path.is_file():
                with path.open("rb") as content:
                    stream.addfile(info, content)
            else:
                stream.addfile(info)
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(
        f"{checksum}  {archive.name}\n"
    )
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-kit", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    options = parser.parse_args()
    try:
        print(prepare(options.base_kit, options.output))
    except (
        ValueError,
        OSError,
        module.InstallError,
        subprocess.CalledProcessError,
    ) as error:
        parser.exit(1, f"Installation kit not created: {error}\n")


if __name__ == "__main__":
    main()
