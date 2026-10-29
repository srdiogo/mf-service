# mf-service

Planned user-session service for the MiniFuse Linux control platform.

**Project status: architecture and contracts only.** No executable, installed
unit or running daemon is supplied yet. This repository does not open a device
or change a configuration. Its implementation is milestone M2/M3 of the shared
platform plan in the `mf-cli` repository, `docs/platform/DELIVERY.md`.

## Purpose

Give the desktop application and Omarchy plugin one consistent view of device
state and one coordinated operation queue. Reuse the future `mf-core` library
from `mf-cli`; do not implement USB selectors, gain conversion, snapshot restore
or rollback a second time here.

The core is a library. This service hosts it in an ordinary user process and
adds discovery, a local API, subscriptions and lifecycle handling. It does not
launch a GUI, choose a desktop, load a module or install access rules.

## Initial release boundary

- Existing mapped MiniFuse 4 mixer, routing and host configuration operations.
- Explicit capabilities and meaningful unavailable/unsupported states.
- Bounded, versioned JSON over a user-owned Unix socket.
- Full-state conflict checks, durable operation identity and verified outcomes.
- Optional generic button event only after physical validation.

MF1/MF2 mixer operations remain unsupported. Preserve their legacy CLI support
in `mf-cli`; service v1 does not pretend those toggles have verified readback.
Adding model-specific legacy operations is a separately tested capability.

Effects, firmware/device-storage writes, arbitrary control transfers and host
audio routing changes are outside scope. Device readback does not prove that
the interface has completed nonvolatile saving.

## Documents

- [Architecture and lifecycle](docs/ARCHITECTURE.md)
- [Draft IPC v1 contract](docs/IPC.md) — canonical wire specification
- [Implementation and validation gates](docs/IMPLEMENTATION.md)

## Dependencies and delivery

Implementation will require `mf-core` extracted from baseline `mf-cli`
`7e67718bd033a59473407e60f92dce1922a82745`, the matching existing kernel module
and administrator-provided device access. The extraction has not happened yet.
Pin a reviewed core Git revision until a library release exists.

The intended runtime is Linux with a graphical user session; a GUI toolkit is
not a service dependency. A user service/unit and package will be reviewed in a
later milestone. No `sudo`, Polkit prompt or system-wide daemon belongs in the
ordinary control path.

New project-owned material is [MIT licensed](LICENSE). Protocol provenance and
upstream credits remain in the `mf-cli` evidence documentation. This is an
independent community project, not an official Arturia application.
