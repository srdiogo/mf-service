#!/usr/bin/env python3
"""Read-only MiniFuse installation checks; never installs or changes settings."""

import argparse
import json
import os
import platform
import shutil
import socket
import stat
import struct
from pathlib import Path

FRAME_LIMIT = 256 * 1024


def owned(path, uid, kind, mode):
    info = path.lstat()
    return (
        info.st_uid == uid and kind(info.st_mode) and stat.S_IMODE(info.st_mode) == mode
    )


def probe(runtime, uid):
    """Only hello/list_devices are sent; do not expose identities or state."""
    directory = runtime / "mf-service"
    endpoint = directory / "control.sock"
    if not owned(runtime, uid, stat.S_ISDIR, 0o700):
        raise ValueError("XDG_RUNTIME_DIR must be an owned private directory")
    if not owned(directory, uid, stat.S_ISDIR, 0o700):
        raise ValueError("The service directory must be owned with mode 0700")
    if not owned(endpoint, uid, stat.S_ISSOCK, 0o600):
        raise ValueError("The service socket must be owned with mode 0600")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(3)
        connection.connect(str(endpoint))
        credentials = connection.getsockopt(
            socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")
        )
        if struct.unpack("3i", credentials)[1] != uid:
            raise ValueError("The service peer belongs to another user")
        stream = connection.makefile("rb")
        with stream:

            def call(method, params):
                connection.sendall(
                    json.dumps(
                        {"api": 1, "id": method, "method": method, "params": params}
                    ).encode()
                    + b"\n"
                )
                frame = stream.readline(FRAME_LIMIT + 1)
                if not frame.endswith(b"\n") or len(frame) > FRAME_LIMIT:
                    raise ValueError("Invalid or oversized service frame")
                response = json.loads(frame)
                if (
                    not isinstance(response, dict)
                    or response.get("api") != 1
                    or response.get("id") != method
                ):
                    raise ValueError("Unexpected service response")
                result = response.get("result")
                if not isinstance(result, dict) or "error" in response:
                    raise ValueError("The service rejected a diagnostic read")
                return result

            hello = call("hello", {"client": "minifuse-doctor", "supported_api": [1]})
            if hello.get("selected_api") != 1:
                raise ValueError("The service does not support API v1")
            devices = call("list_devices", {}).get("devices")
            if not isinstance(devices, list) or any(
                not isinstance(device, dict) for device in devices
            ):
                raise ValueError("Invalid service inventory")
    mf4 = [device for device in devices if device.get("product_id") == 0xAF70]
    ready = len(mf4) == 1 and mf4[0].get("connection") == "ready"
    capabilities = mf4[0].get("capabilities", {}) if ready else {}
    features = ("mixer", "switch_readback", "input_switches", "signal_meters")
    return {
        "api_major": 1,
        "device_count": len(devices),
        "mf4_ready": ready,
        "verified_features": isinstance(capabilities, dict)
        and all(capabilities.get(name) is True for name in features),
    }


def diagnose(root=Path("/"), env=None, uid=None, kernel=None, no_service=False):
    env = os.environ if env is None else env
    uid = os.getuid() if uid is None else uid
    kernel = platform.release() if kernel is None else kernel
    checks = []

    def add(name, ok, message, required=True):
        checks.append(
            {
                "check": name,
                "status": "ok" if ok else ("error" if required else "optional"),
                "message": message,
            }
        )

    node = root / "dev/minifuse_cmd"
    add("driver", (root / "sys/module/minifuse_mod").is_dir(), "minifuse_mod loaded")
    add(
        "device_access",
        node.exists() and os.access(node, os.R_OK | os.W_OK),
        "Current user can read/write the control node",
    )
    add(
        "kernel_headers",
        (root / f"usr/lib/modules/{kernel}/build/Makefile").is_file(),
        "Headers match the running kernel (needed for DKMS builds)",
        False,
    )
    add(
        "service_binary",
        shutil.which("mf-service", path=env.get("PATH", "")) is not None,
        "mf-service is available in PATH",
    )
    add(
        "gui",
        shutil.which("minifuse-control", path=env.get("PATH", "")) is not None,
        "Optional full mixer is available in PATH",
        False,
    )
    add(
        "omarchy",
        shutil.which("omarchy", path=env.get("PATH", "")) is not None,
        "Optional Omarchy host is available in PATH",
        False,
    )
    if not no_service:
        try:
            runtime = Path(env.get("XDG_RUNTIME_DIR", ""))
            if not runtime.is_absolute():
                raise ValueError(
                    "Use the owning graphical login session with XDG_RUNTIME_DIR"
                )
            result = probe(runtime, uid)
            add("service_api", True, "Connected to API v1")
            add(
                "mf4_ready", result["mf4_ready"], "Exactly one MiniFuse 4 must be ready"
            )
            add(
                "verified_features",
                result["verified_features"],
                "Mixer, input readback/writes and meters must be available",
            )
            add(
                "inventory",
                result["device_count"] > 0,
                f"Service reports {result['device_count']} device(s)",
            )
        except (OSError, ValueError) as error:
            add("service_api", False, f"Service unavailable: {error}")
    return {
        "schema_version": 1,
        "checks": checks,
        "ok": all(item["status"] != "error" for item in checks),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print a report without device identities or settings",
    )
    parser.add_argument(
        "--no-service", action="store_true", help="Skip the read-only session API probe"
    )
    args = parser.parse_args()
    report = diagnose(no_service=args.no_service)
    if args.json:
        print(json.dumps(report, indent=2))
    else:
        for item in report["checks"]:
            print(f"{item['status'].upper()}: {item['check']}: {item['message']}")
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
