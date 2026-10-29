# Implementation and validation gates

## Work sequence

1. Depend on the reviewed extracted `mf-core`; compile a real public-API
   consumer. Select and pin a maintained Rust socket/runtime/serialization stack
   whose compiler requirements are recorded. Do not copy private core modules.
2. Implement strict framing/DTOs and contract fixtures, then the coordinator
   against a deterministic fake backend used only in tests.
3. Implement the bounded blocking worker, plans and durable operation identity.
4. Add descriptor discovery, MF4 access, state subscriptions and lifecycle.
5. Add preset storage and explicit content import/export boundaries.
6. Validate on hardware with recorded baseline/restore and controlled audio.
7. Ship reviewed package/user-unit files with documented optional enablement.

The foreground implementation and locked `mf-core` dependency are present.
Coordinator, persistence and real Unix-socket tests use deterministic devices
only in tests. Hardware, two-UI-client and clean-machine release checks remain
open; passing software tests does not complete those gates.

## Automated acceptance

- Correct UTF-8/LF framing across fragmented/combined reads; frame deadline,
  maximum size, duplicate JSON keys and malformed request rejection.
- No hardware/backend invocation for invalid requests, unsupported models,
  offline inspection, expired plans, mismatched identity or backup failures.
- Plan/apply conflict tests, including changes by a direct external client.
- Two clients receive consistent baselines and sequence-ordered state changes.
- Slow subscribers cannot block device work or grow memory without bound.
- Queue overload is explicit; queued work is invalidated on device generation
  change. No operations leak across reconnection or service restart.
- Same idempotency key returns/query identifies the original operation; changed
  content is rejected. Crash-before/after-dispatch records do not cause replay.
- Verified write plus journal-finalization failure remains distinguishable.
- Rollback flags and complete state are preserved from core outcomes; attempted
  recovery alone is never reported as success.
- Presets are exclusive mode-0600 files, bounded and schema/model checked.
  Snapshot paths/traversal strings cannot escape the designated store.
- Ordinary requests never invoke a shell, request privilege, detach/reset
  `snd-usb-audio`, load a module or write firmware/storage.

## Hardware and desktop acceptance

Use one known MF4, record the tested source revision/binary/module hashes,
initial switch state and full B2 baseline. Start with reads, dry runs and muted
sources. Validate mapped control changes through existing full-state guards.
Restore all mapped state and separately restore any explicitly tested legacy
toggles; compare the entire 190-byte payload before ending.

Run the actual service with both GUI/plugin clients, then a direct CLI change.
Observe stale-work behavior without claiming the service owns every possible
writer. Removal during execution is a separate controlled test; it may prevent
rollback and must not be described as a successful recovery automatically.

Keep microphone PCM ephemeral when only measuring levels. Publish synthetic
signals/anonymous measurements, not user snapshots, serials or private host
logs. Do not cycle power/phantom solely to complete an unrelated software test.

## Release blockers and follow-ups

The initial release requires the core extraction, contract tests, capability
gates, durable outcomes, bounded queues, reconnect handling and a tested package.
Logo-button decoding remains unavailable. MF4 signal meters are calibrated and
implemented as ephemeral subscriptions;
neither may be represented by mock data in a shipping UI. Full MF1/MF2 GUI
controls require their own evidence and hardware regression checks.

The service starts by explicit user/session configuration, not by changing
system audio. Document socket location, access setup, foreground diagnostics,
user-unit startup/shutdown and complete package removal. Verify a clean install
without enabling any desktop integration unless the user selects it.

## Continuous interaction coverage

The implementation includes fixed-target live controls with initial snapshots,
durable sample intent, grouped core evidence, bounded samples/lease and final
settlement. Tests cover intermediate/final gains, idempotency, invalid values,
external complete-state changes, exclusive admission, cancellation, client
loss, process restart and pre-dispatch metadata failure. This software coverage
does not establish a hardware update rate or physical-switch readback.
