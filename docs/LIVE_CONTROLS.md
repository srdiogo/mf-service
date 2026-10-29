# Continuous mixer interaction

The desktop clients read state on connection and follow subscriptions. Normal
mixer interaction has no Read/Write workflow: faders and pan apply during the
gesture, mute/solo/routes apply on deliberate interaction, and the final fader
value is submitted on release. Manual refresh is a diagnostic action. Snapshot
restore retains its separate comparison and explicit Apply step.

## Service boundary

Continuous controls use one durable logical operation per gesture, advertised
as `live_controls` in negotiation. A fixed target is selected at the beginning:
channel level, channel pan or mix master. It cannot become a raw selector or
change targets midway. Identity, generation and initial revision are checked
before admission. At most one live gesture is admitted for the interface.

- `begin_control`: device/generation/revision, immutable typed control target,
  and client-scoped idempotency key. Returns the durable operation ID.
- `update_control`: operation ID, monotonic sequence, typed value, and `finish`.
  Duplicate identical samples return the original admission receipt; changed reuse fails.
- `cancel_control`: stops admission of further samples, without undoing already
  verified changes or applying an unsent value.
- `control_updated`: the admitted sample has settled with its verification facts.
  The final sample emits the ordinary `operation_finished` event instead.

A client has at most one sample in flight and one replaceable latest desired
value. Pointer events render immediately; hardware submissions are coalesced
with an initial 80 ms minimum interval. This is a scheduling setting, not a
latency guarantee. Release preserves the latest value and marks the final
sample; it cannot disappear behind an older queued value. No request is
replayed automatically after a timeout/disconnection.

Each dispatched sample still uses the unchanged core's durable pre-write
snapshot/journal and complete-state kernel verification. Its records share a
private per-gesture directory. The service records sample intent before calling
the core, updates the one gesture identity after settlement, and retains the
initial baseline. Grouping service identities does not bypass per-write sync,
weaken rollback or promise atomic USB changes.

Samples are bounded to 512 per gesture, with a 30-second inactivity lease. A
closed client, cancellation, expiry or conflict ends the gesture without
replaying pending UI data. Other service mutations wait/reject while it owns
the interface; direct external writers remain possible and trigger full-state
conflict checks. Every sample is checked against the last confirmed complete
state. An external or physical change stops the old gesture instead of being
silently overwritten.

A prepared operation after a crash remains indeterminate. A verified last
sample and a completed user gesture are separate facts. Recovery shows whether
the gesture finished, was interrupted, or has an uncertain outcome. It never
claims that an unsent final position reached the device.

## Switch synchronization

INST/+48 V indicators must come from validated, model-specific readback.
Physical changes update those indicators through the same state subscription;
remembering the last software command is insufficient. This remains a separate
evidence gate. No firmware change is needed or introduced by this interaction
model.
