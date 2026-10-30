# Architecture and lifecycle

Status: implemented development architecture; hardware/release gates remain open.

## Process model

The library and service are separate Cargo crates in one workspace. The
library is linked into the service; this creates no additional process, IPC hop
or polling loop. The Linux transport is versioned in the same repository.
The CLI is an independent optional consumer, not an installation prerequisite.

One service runs per user session. A socket acceptor/API task validates and
dispatches requests; a dedicated blocking worker performs discovery, state
reads, journal sync and hardware execution. USB calls never block a GUI event
loop or the service's socket loop.

Start with one addressable, unambiguous MF4. The current kernel read/apply path
refuses multiple MF4 devices; inventory may report them, but the service must
not silently choose one. A device handle is a session token, not a USB address
or a claim of serial-bound identity.

Source ownership (coordination/discovery/outcomes currently share one module):

```text
crates/mf-core/      synchronous library; no daemon or desktop dependencies
crates/mf-service/src/
  main.rs             foreground lifecycle and signal handling
  server.rs           socket admission, framing deadlines and bounded writers
  protocol.rs         strict envelopes, domain DTOs and error codes
  coordinator.rs      revisions, plans, subscriptions and durable outcomes
  live.rs             fixed-target gestures, sample intents and settlement
  hardware.rs         descriptor discovery and real mf-core adapter
  storage.rs          exclusive private records and atomic finalization
crates/mf-service/tests/
  coordinator.rs      fake hardware, invariants and canonical fixture check
  server.rs           real local sockets, framing, deadlines and backpressure
tests/contract/v1.json  shared service/client contract fixture
kmod/                bounded GPL-2.0-only Linux control transport
packaging/           native driver/service packages and optional user unit
```

## Socket and local access

Use `$XDG_RUNTIME_DIR/mf-service/control.sock`, in a mode-0700 owned directory,
with a mode-0600 socket. Require a valid runtime directory owned by the current
UID; do not fall back to a shared predictable `/tmp` socket. Check peer UID
with `SO_PEERCRED`. Bind failure means another instance or an access error;
never unlink another live instance's socket automatically.

Clients are local applications of the same user, not a sandbox boundary against
other code already running as that user. Bound messages, work queues and
subscriber buffers. Do not accept shell commands, raw selectors or arbitrary
service-side file paths. There is no TCP listener, web API or root service.

## State and connection generations

Assign a new service epoch on process startup and a fresh device generation
after each connection/reconnection. A state revision refers to an exact full
B2 payload within that epoch/generation. A client must obtain a full baseline
before making a plan; a USB bus/address alone is insufficient identity.

Poll conservatively through the existing backend and measure a suitable
cadence before enabling frequent status updates. Idle views reduce polling.
Publish only meaningful state changes. Do not treat B2 reads as meter samples.
Unsupported products may be discovered through descriptors but receive no
inferred B2 query. Capability evidence is tied to model/layout and testing.

## Plans, queueing and persistence

1. A high-level request is validated by the core against current state.
2. Return an opaque, immutable plan ID, expected revision, predicted result,
   omitted snapshot differences and a 30-second monotonic expiry.
3. On apply, check epoch, device generation, plan ownership/expiry and expected
   revision. Serialize through one bounded queue (initial limit: 32 pending
   operations). Reject overload explicitly; do not accumulate slider movement.
4. Assign/persist operation identity with the existing prepared backup/journal
   before a hardware call. The kernel's expected complete-state check remains
   authoritative even if the service's cached revision matched.
5. Publish the verified state or structured failure, including rollback and
   journal outcomes. Refresh state before permitting further work after an
   uncertain outcome.

Continuous fader/pan gestures use a fixed typed target and one durable service
operation identity. Each sample still runs through unchanged core backup,
journal, full-state verification and rollback. Clients coalesce pointer input
into one in-flight sample plus one latest desired value. See the exact
[continuous-control contract](LIVE_CONTROLS.md). Direct CLI/external writers
remain possible and trigger conflict/readback guards.

Idempotency keys are scoped to an explicit client token and canonical request
digest. Reusing a key with a different request is an error. Persist the key and
operation identity before dispatch so a reconnect cannot unknowingly repeat a
write. After a crash, a prepared record is indeterminate; do not replay it.
Report that status and require refresh/reconciliation. Bound retained records
with a documented policy that never deletes evidence for unresolved work.

## Disconnection and shutdown

Client connection loss invalidates its unsubmitted plans and ends live gestures.
Already admitted ordinary operations retain their durable identities and may
settle after the caller disconnects. Device connection loss invalidates plans. If removal happens after dispatch, report the actual known result;
do not claim rollback when it could not be verified. Reconnection creates a
new generation and reads a baseline; it never restores presets automatically.

Graceful shutdown stops admission, rejects queued work and allows a started
transaction to settle within a bounded shutdown budget. Do not imply that
terminating a userspace process cancels an ioctl already executing in the
kernel. Preserve prepared records for interrupted/unknown outcomes.

Subscriptions have bounded buffers (initial target: 64 events per client).
Disconnect a slow client with an overflow reason rather than losing state
events silently. It must reconnect and fetch a full baseline. No historical
event replay is promised in v1.

## Storage and runtime activation

Keep existing operation backups under `$XDG_STATE_HOME/mf-cli` (or its existing
fallback), so direct/service operations share one documented evidence location.
Store service metadata separately under `$XDG_STATE_HOME/mf-service`; never
rename or reinterpret old CLI journals silently. Any journal extension requires
compatible readers and explicit format tests.

Named user presets belong under `$XDG_DATA_HOME/minifuse/presets`; runtime
preferences belong under `$XDG_CONFIG_HOME/minifuse`. Use normal XDG fallbacks.
Reject invalid/untrusted preset IDs. Import/export explicit snapshot content
instead of accepting arbitrary service-side paths from clients.

The service can start on demand for a UI. To observe the physical button while
all UIs are closed, it must remain active with the optional desktop agent during
that login session. Service installation/enablement is a separate packaging
step. Disconnecting the device never enables startup or changes system audio.

## Button boundary

The core may eventually decode a validated physical-button signal; the service
publishes `control_panel_requested`. It does not choose an application, run a
command or depend on Omarchy. Keep that capability unavailable until short
release, wraparound, reconnect and long-press behavior have been measured.
Rebaseline on startup; counter contents alone must not activate a UI.


## Current bounds and retention

The implementation uses the standard Rust 1.85 Unix-socket/thread stack with
Serde/serde_json/libc, pinned in `Cargo.lock`. There are at most 16 connections,
32 admitted request slots, 32 pending operations, 64 outbound frames per client,
128 plans total/eight per client, 4096 retained operations, 4096 client identities
and 256 presets. A full store rejects admission; it never deletes unresolved
evidence automatically. Archive resolved records explicitly while stopped.

Polling is one second with subscribers and five seconds otherwise, plus reads
for explicit state/planning/dispatch. These are initial implementation settings,
not measured latency/stream-continuity claims. Descriptor identity includes the
sysfs connection inode; session identity still is not serial-bound. MF1/MF2
inventory does not issue B2 queries. Multiple MF4s remain ambiguous.

Graceful stop ceases admission and allows ten seconds for the worker to settle.
Expiry of that userspace budget cannot cancel a dispatched kernel ioctl. The
process reports the limitation and retains durable prepared records; a restart
classifies them as indeterminate and never replays them. The optional user unit
allows twelve seconds before systemd's stop policy applies.
