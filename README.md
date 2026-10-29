# mf-service

A Rust user-session service for the [MiniFuse Linux control platform](https://github.com/srdiogo/mf-cli). It gives
the standalone application and Omarchy plugin one capability-driven view of
device state and a serialized, verified operation path through `mf-core`.

**Status: development preview with bounded MiniFuse 4 hardware validation.**
The foreground server, Linux discovery, API, plans, durable operation identities,
subscriptions and host presets are implemented. MF4 input switches and calibrated signal-meter subscriptions are implemented.
Logo-button events and MF1/MF2 mixer operations remain unavailable. Clean-machine and broader hardware acceptance remain open. Building or testing
does not enable a unit or desktop startup.

## Build and run

Requires Linux, Rust 1.85+ and the exact core revision in `Cargo.toml`/`Cargo.lock`.
The service uses Rust's Unix sockets/threads, Serde, serde_json and libc; Qt and
libusb are not runtime dependencies. Normal control requires the matching
administrator-installed MiniFuse module and existing session access.

```sh
cargo build --release --locked
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
./target/release/mf-service --help
./target/release/mf-service
```

The server uses `$XDG_RUNTIME_DIR/mf-service/control.sock` (0700 directory,
0600 socket), checks peer UID and refuses to replace a live instance. It never
falls back to a shared `/tmp` socket, starts as root, loads a module, detaches an
audio driver or restores a preset automatically. Use Ctrl+C to stop it.

The service currently pins `mf-core` commit
`936962fcef9320fbf0e0991bd095d9c6c9370dc4`. The initial shared core was integrated by
[the shared-core PR](https://github.com/srdiogo/mf-cli/pull/2). USB semantics,
gain conversion, snapshot restore and rollback have one implementation there.

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
credits remain in the `mf-cli` evidence record.

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
belong in [mf-core](https://github.com/srdiogo/mf-cli).
