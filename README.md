# mf-service

A Rust user-session service for the MiniFuse Linux control platform. It gives
the standalone application and Omarchy plugin one capability-driven view of
device state and a serialized, verified operation path through `mf-core`.

**Status: implemented development version; not a hardware-validated release.**
The foreground server, Linux discovery, API, plans, durable operation identities,
subscriptions and host presets are implemented. Physical-button events, meters
and MF1/MF2 mixer operations remain unavailable. No unit or desktop startup is
enabled by building or testing this repository.

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
`4775fd23e0bff5b28bbd138e4108dc8e519a0021`, integrated by
[the shared-core PR](https://github.com/srdiogo/mf-cli/pull/2). USB semantics,
gain conversion, snapshot restore and rollback have one implementation there.

## API and storage

- [API v1 candidate and exact result shapes](docs/IPC.md)
- [Architecture, bounds and lifecycle](docs/ARCHITECTURE.md)
- [Implementation status and validation](docs/IMPLEMENTATION.md)
- [Installation and troubleshooting](docs/INSTALL.md)
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
