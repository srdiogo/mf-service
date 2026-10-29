# mf-service

The backend for the MiniFuse Linux control platform. This repository contains
the `mf-core` Rust library, the `mf-service` user-session process and the Linux
control driver. It gives
the standalone application and Omarchy plugin one capability-driven view of
device state and a serialized, verified operation path through `mf-core`.

**Status: development preview with bounded MiniFuse 4 hardware validation.**
The foreground server, Linux discovery, API, plans, durable operation identities,
subscriptions and host presets are implemented. MF4 input switches and calibrated signal-meter subscriptions are implemented.
Logo-button events and MF1/MF2 mixer operations remain unavailable. Clean-machine and broader hardware acceptance remain open. Building or testing
does not enable a unit or desktop startup.

## Build and run

Requires Linux, Rust 1.85+, a C compiler for ABI tests and the committed
`Cargo.lock`. Both Rust crates are built from this workspace.
The service uses Rust's Unix sockets/threads, Serde, serde_json and libc; Qt and
libusb are not runtime dependencies. Normal control requires the matching
administrator-installed MiniFuse module and existing session access.

```sh
cargo build --release --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
./target/release/mf-service --help
./target/release/mf-service
```

The server uses `$XDG_RUNTIME_DIR/mf-service/control.sock` (0700 directory,
0600 socket), checks peer UID and refuses to replace a live instance. It never
falls back to a shared `/tmp` socket, starts as root, loads a module, detaches an
audio driver or restores a preset automatically. Use Ctrl+C to stop it.

## Components and installation

| Component | Role | Runtime |
|---|---|---|
| `crates/mf-core` | Typed state, capability gates, planning, verified transport and host snapshots | Compiled into the service |
| `crates/mf-service` | Shared API, serialization, subscriptions and durable operation identities | One process per user session |
| `kmod` | Bounded Linux control transport; keeps `snd-usb-audio` bound | `minifuse_mod` kernel module |
| [MiniFuse Control](https://github.com/srdiogo/minifuse-control) | Full mixer application | Optional separate project |
| [Omarchy MiniFuse](https://github.com/srdiogo/omarchy-minifuse) | Bar widget and compact mixer | Optional separate project |

The CLI is not required to build, install or run this backend. The core is a
library, not another background service. GUI and plugin share the same service.
The wire protocol remains API v1 and existing snapshots retain schema 1.

Arch/Omarchy packages are `minifuse-driver-dkms` and `mf-service`. Driver setup
requires matching kernel headers and administrator access; normal control runs
as the logged-in user. See [installation from a clean machine](docs/INSTALL.md),
[package builds](packaging/arch/README.md) and the read-only `minifuse-doctor`.

## API and storage

- [API v1 candidate and exact result shapes](docs/IPC.md)
- [Architecture, bounds and lifecycle](docs/ARCHITECTURE.md)
- [Implementation status and validation](docs/IMPLEMENTATION.md)
- [Installation and troubleshooting](docs/INSTALL.md)
- [Hardware integration validation](docs/MF4_VALIDATION.md)
- [Contribution workflow](CONTRIBUTING.md)
- [Canonical service/client fixtures](tests/contract/README.md)

First negotiate `hello`, then obtain inventory and a confirmed state baseline.
Faders/pan use bounded continuous gestures through `begin_control` and
`update_control`; discrete changes use `plan_changes`/`apply_plan`. Snapshot
restore retains its comparison and explicit apply. See [live controls](docs/LIVE_CONTROLS.md). Accepted
operations have durable IDs; completion is queried or delivered by event.
A reconnect or timeout never permits automatic replay. Keep the client token
and idempotency key when reconciling an uncertain request.

Operation identities are under `$XDG_STATE_HOME/mf-service`, existing core
backups under `$XDG_STATE_HOME/mf-cli`, and presets under
`$XDG_DATA_HOME/minifuse/presets`, with standard home-directory fallbacks.
Snapshots are host files. Readback does not confirm nonvolatile saving.

Effects, firmware/device-storage writes, raw USB/selectors and host audio
configuration changes are outside scope. This is an independent community
project. New material is [MIT licensed](LICENSE); upstream/protocol research
credits are preserved in [the evidence record](docs/mf4/evidence.md) and
[third-party notices](THIRD_PARTY.md). The kernel code is GPL-2.0-only; see
[kmod/LICENSE](kmod/LICENSE).

MF4 input switch state is read from the validated B2 fields. Typed INST1,
INST2 and shared phantom-power changes use the same durable plan/apply path
as mixer changes. Writes require the active module to advertise verified
input-switch transactions; old modules retain readback without these writes.
Mixer preset restoration never changes input mode or phantom power.

Signal meters are read-only, subscriber-driven ephemeral events with a target
ceiling of 20 Hz. They do not change state revisions or create journals. Mix
peaks are after master gain and before destination routing; they can differ
from physical output levels when USB direct bypasses the custom mix.

The standalone [MiniFuse Control GUI](https://github.com/srdiogo/minifuse-control)
is a separate project. Clients share this service; protocol and kernel changes
belong in this workspace.
