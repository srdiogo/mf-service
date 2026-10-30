#!/usr/bin/env python3
"""Export a pinned, checksummed Arch source kit from a clean Git checkout."""

import argparse
import gzip
import hashlib
import io
import json
import re
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def git(*args, root=ROOT):
    return subprocess.check_output(["git", "-C", str(root), *args])


def export(output, revision="HEAD", root=ROOT):
    if git("status", "--porcelain", root=root).strip():
        raise ValueError("Commit the source changes before exporting a release kit")
    commit = (
        git("rev-parse", "--verify", f"{revision}^{{commit}}", root=root)
        .decode()
        .strip()
    )
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("Expected a complete Git commit")
    manifest = tomllib.loads(
        git("show", f"{commit}:crates/mf-service/Cargo.toml", root=root).decode()
    )
    version = manifest["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("Expected a release version with three numeric components")
    count = git("rev-list", "--count", commit, root=root).decode().strip()
    package_version = f"{version}.r{count}.g{commit[:7]}"
    archive_name = f"mf-service-{commit}.tar.gz"
    tar = git(
        "archive", "--format=tar", f"--prefix=mf-service-{commit}/", commit, root=root
    )
    compressed = io.BytesIO()
    with gzip.GzipFile(filename="", mode="wb", fileobj=compressed, mtime=0) as stream:
        stream.write(tar)
    archive = compressed.getvalue()
    checksum = hashlib.sha256(archive).hexdigest()
    recipe = git("show", f"{commit}:packaging/arch/PKGBUILD.in", root=root).decode()
    recipe = (
        recipe.replace("@VERSION@", package_version)
        .replace("@COMMIT@", commit)
        .replace("@SHA256@", checksum)
    )
    if re.search(r"@[A-Z0-9_]+@", recipe):
        raise ValueError("Unresolved package template field")
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError("The output directory must be empty")
    (output / archive_name).write_bytes(archive)
    (output / "PKGBUILD").write_text(recipe)
    profile = {
        "schema_version": 1,
        "repository": "https://github.com/srdiogo/mf-service",
        "commit": commit,
        "package_version": f"{package_version}-1",
        "api_major": 1,
        "files": {
            archive_name: checksum,
            "PKGBUILD": hashlib.sha256(recipe.encode()).hexdigest(),
        },
    }
    (output / "source-lock.json").write_text(json.dumps(profile, indent=2) + "\n")
    (output / "SHA256SUMS").write_text(
        "".join(f"{value}  {name}\n" for name, value in profile["files"].items())
    )
    return profile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--revision", default="HEAD")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        profile = export(args.output, args.revision)
    except (ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Source kit not created: {error}\n")
    print(f"Source kit: {args.output} ({profile['package_version']})")


if __name__ == "__main__":
    main()
