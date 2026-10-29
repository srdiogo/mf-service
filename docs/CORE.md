# Shared MiniFuse control library

`crates/mf-core` is the synchronous Rust library linked into `mf-service` and available to other typed consumers. It owns
model capabilities, state decoding, gain conversion, domain planning, the
existing kernel transport and durable host snapshots. It does not start a
service, load a module, detach an audio driver or choose a desktop application.

## Public API

An external consumer can inspect a snapshot and propose an operation offline:

```rust
use mf_core::{Level, Mix, Operation, Snapshot};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
let saved = Snapshot::load(Path::new("studio.json"))?;
let before = saved.state()?;
let proposed = mf_core::plan(&saved.device, &before, &[
    Operation::Master { mix: Mix::Out12, level: Level::Db(-12.0) },
])?;
assert_eq!(proposed.before(), &before);
// Planning never opens a device. A live operation must use freshly read state.
Ok(())
}
```

The domain operations are `Channel`, `Master`, `Route`, `Membership` and
`InputSwitch`. The last accepts only `Inst1`, `Inst2` or `PhantomPower` and a
boolean. Phantom power is shared by the two physical preamps on MiniFuse 4.
`Level::Silence` is distinct from a finite dB value. An absent channel field
preserves it. Overlapping selectors are rejected before unchanged fields are
filtered, including overlaps with membership removal's mute/solo effects.

`Plan` cannot be constructed or edited by consumers. It binds the USB device
record, complete initial payload, bounded changes and predicted final payload.
`plan_restore` accepts a validated schema-1 snapshot and an optional mix scope;
`omitted_offsets` reports bytes outside that restore scope. `summary` retains
the existing CLI diagnostic JSON, but it is not an execution input format.
There is no arbitrary-selector or raw-state-write public method.

`Kernel::open(false)` and `read` provide confirmed state. Execution uses a
write-capable kernel handle and `execute(&mut backend, &plan, backup_directory)`.
No userspace USB fallback is used for mixer operations. `Backend` provides the
same typed transaction boundary to deterministic tests and the real kernel.
The kernel remains the final full-state/device-identity guard against external
writers; a library plan does not grant exclusive ownership of the interface.

## Errors and execution outcomes

`ErrorKind` distinguishes invalid input, unsupported capability, access,
ambiguous/missing device, missing/incompatible module, changed identity, stale
state, backup, transfer, readback and journal failures. Transport classification
uses operating-system errors and kernel flags, never parsed terminal wording.
`device_write_attempted = None` means dispatch cannot be established reliably.

Preparation errors return before a device mutation. After dispatch, `Outcome`
preserves the device result, confirmed state if available, rollback attempted
and verified flags, journal status, backup and operation paths. A rollback is
verified only when its flag and complete returned state agree with the original
payload. An attempted rollback is not a successful recovery.

A verified device write can coexist with `JournalStatus::Failed`. The durable
prepared record remains available and must not be replayed. `require_success`
is the terminal adapter: it reports that combination as `JournalFailed` while
retaining its confirmed device state. Service/UI callers should inspect the
separate outcome fields instead of collapsing them into one boolean.

## Compatibility and dependencies

The workspace retains Rust 1.85, legacy model maps,
snapshot schema 1, exclusive mode-0600 files, pre-write sync and the existing
206-byte status/1032-byte transaction ABI. The additive 16-byte module-capability ioctl advertises verified input-switch
transactions. An older module returns no additional capabilities; the transport
rejects input writes before dispatch. Mixer transactions keep their existing
ABI and behavior. Journals retain their existing status and fields, with additive
device-outcome/recovery fields. Input writes preserve every unknown header bit. Snapshot restore still excludes
INST and phantom power, preventing a saved mixer preset from changing microphone
power or input mode. Trims, topology restrictions and unvalidated controls retain
their previous behavior.

MF1/MF2 advertise no mixer/snapshot capability and never receive inferred B2
queries. MF4 decodes the three input switches from B2; MF1/MF2 do not inherit those
fields or selectors. MF4 signal meters use a fixed read-only C2 query when the active module
advertises it. Physical-logo-button events remain unavailable.
Capabilities describe potentially supported membership; the planner also checks
the actual topology. Device readback still does not prove nonvolatile saving.

The crate is not published to crates.io. Workspace consumers use a path
dependency; independent repositories must pin a reviewed Git revision and
package `mf-core`. Its only dependencies are Serde, serde_json and libc from
the committed lockfile. No Clap, Qt or libusb dependency is required by this workspace.

## Validation

`cargo test --workspace --locked` runs the model/fixture and
C/Rust conformance tests, plus an integration consumer using only the public library API. The
consumer covers immutable planning, model gates, overlap rejection, recovery
facts and journal-finalization failure after verified execution. CI also runs
formatting, Clippy, release builds and the independent kernel build on the
minimum Rust toolchain and current stable where applicable.

These offline checks exercise the software boundary. The existing hardware
report remains tied to its recorded source/binary hashes; extraction alone
does not constitute another hardware test.

Input-switch tests cover exact bit preservation, reverse rollback values,
overlap rejection, restore exclusions, MF1/MF2 gating, C/Rust ABI agreement and
rejection against a module without the new feature. Physical readback evidence is
recorded in [the input-switch report](mf4/input-switches.md).

## Signal meters

`Kernel::meters()` returns the USB device record and typed `Meters`, independently
of configuration state. The 64-byte C2 payload contains 16 little-endian unsigned
peak magnitudes with full scale `2^31`: Analog 1–4, USB 1–6, then the left/right
outputs of out12, out34 and loopback mixes. `sources_dbfs` and `mixes_dbfs` use
`20 * log10(magnitude / 2^31)`; zero is `None`, serialized as null. Values above
full scale and malformed headers are rejected. Mix peaks follow master gain
and precede destination route selection; they are not guaranteed to represent
physical outputs when a destination bypasses its custom mix.

`ModuleCapabilities.signal_meters` gates the additive 80-byte read-only ioctl.
Older modules do not advertise it. MF1/MF2 do not inherit this layout. There are
no meter writes, snapshots, journals or firmware changes. See
[calibration and coverage](mf4/meters.md).
