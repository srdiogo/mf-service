# MiniFuse 4 input switches

## Scope

The B2 readback fields below were validated on one MiniFuse 4 (`1c75:af70`)
on 29 October 2026 by comparing physical LED confirmations with fixed-length
190-byte state reads. This does not establish MF1/MF2 layouts or compatibility
with every MF4 firmware. No firmware modification was involved.

| Physical transition | Changed byte | Before | After |
|---|---:|---:|---:|
| INST1 off to on, INST2 off | 0 | 0 | 1 |
| INST2 off to on, INST1 on | 0 | 1 | 3 |
| Both INST switches returned off | 0 | 3 | 0 |
| Shared +48 V on to off, both INST off | 6 | 1 | 0 |
| Shared +48 V returned on | 6 | 0 | 1 |

Only the stated byte changed at each step. After both INST switches were
returned off and phantom power on, all 190 bytes matched the initial payload.
A phantom-powered microphone was connected to Analog 1; the operator was asked
to silence the speakers before cycling phantom power. No PCM or private state
payload is included in this report.

## Implementation boundary

`State::inputs()` decodes word 0 bits 0/1 and word 6 bit 0. Typed `InputSwitch`
operations use the existing model-specific selectors `0000`, `0001`, `0300`.
The full-state transaction predictor preserves all unrelated bits, validates
boolean values and derives inverse values for rollback. The kernel must
advertise the additive input-switch capability before the shared transport
will dispatch these operations. Legacy CLI maps for MF1/MF2 are unchanged.

Mixer snapshots may contain these fields for inspection and comparison, but
restoration never writes input mode or phantom power. Input controls require
an explicit action on the current device. The capability query does not imply
that any device is connected or writable; normal identity/access checks apply.

The physical observations validate readback. The existing legacy-write report
is separate; hardware validation of the extended verified transaction path
must be recorded separately with its tested module artifact. Signal meters
are not configuration gains and are outside this report.

## Verified transaction and GUI test

The extended transaction path was exercised on the same unit later that day,
using core revision `d7a2987b5514db1d3aed2f1e68e2ec534bd7276e`, service revision
`b443e6ae033493d35c90a21db9f7ca7716e1b537` and GUI revision
`488a5cdb393eb43b494a35a4c0b1e1fe83f2d5cd`. The temporarily loaded module had
SHA-256 `5bfd2dbd5c0ad8ab63ad107096cad3a40673310b6b360659d1a6dec15dff3d9a`
and was built for kernel `7.2.5-3-omarchy`. Its capability query advertised
verified input-switch transactions; reloading preserved the complete B2 state.

The operator clicked each INST control on and off and confirmed the corresponding
physical LED. An independent read after enabling INST1 found only byte 0 changed
from 0 to 1. After both INST tests, all 190 bytes matched the baseline. The
operator then disabled shared +48 V in the GUI and confirmed its LED off; only
byte 6 changed from 1 to 0. Re-enabling it restored the LED and all 190 bytes.
The microphone remained connected and the operator was asked to keep speakers
silenced for the phantom-power cycle. Audio driver bindings were retained.

This validates successful input transactions and readback on this unit. It does
not test forced transport failure during rollback, MF1/MF2 hardware or persistence
across a device power cycle. No full private state payload or microphone PCM is
published.
