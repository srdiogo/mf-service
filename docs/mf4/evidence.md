# Evidence, attribution and validation boundaries

## Upstream and prior work

The imported backend comes from `srdiogo/mf-cli` commit
`6456df2be9c0a043c16f57a9d279f25085662741`. The core and driver files at that
revision match the service's former core dependency
`936962fcef9320fbf0e0991bd095d9c6c9370dc4`. They were brought into this workspace
on 29 October 2026 with their original licenses and attribution. Path changes
for offline tests do not establish new hardware validation. The CLI itself is
not included. See [third-party notices](../../THIRD_PARTY.md).

The original CLI fork starts from `nolight132/mf-cli` commit
`08a2aafbccd487d33b9f2b7f44813194f4dfd596` (v0.2.6, 26 October 2026).
The legacy command interface and packaging are retained from that project.
Pavel Olizko and sleebu developed its Linux CLI/kernel transport.

`andreanero`'s [MF4 contribution, PR #6](https://github.com/nolight132/mf-cli/pull/6)
added model detection, MF4 INST per input and phantom power. His
[September working notes](https://github.com/andreanero/mf-cli-mf4-dev/blob/11c2ca399db041681e33549a645feae2a11af319/docs/mf4/NOTES.md)
already described parts of the mixer, read requests, trim link and LED control.
The work here consolidates and extends that research; it does not claim those
controls as independent discoveries. PR #6 was merged on 26 October 2026.

Independent captures and the state/transaction implementation come from
srdiogo's MiniFuse CC Linux investigation (Python/helpers MIT, kernel code
GPL-2.0-only). The Rust implementation is a port of its bounded control model.
Its Windows reference unit reports USB `1c75:af70`, displayed firmware 1.0.1,
MFCC 1.4.2.1381 and Windows driver 1.4.0.0. The displayed version does not
establish its full firmware build number.

## Reproducible evidence

[`control-writes.csv`](control-writes.csv) contains all **712 configuration
writes**, with matched successful USB completions, extracted from 57 capture
files. It includes capture basenames, frame references, setup fields, two-byte
payloads and completion status. It excludes PCM audio, serial numbers, host
identifiers, timestamps, bus/address and host IRP identifiers.

The original public extraction and provenance are available in
[srdiogo's Gist](https://gist.github.com/srdiogo/d11a290c4d38d0d232a6eb9ee871c68d).
SHA-256 checksums for the copies in this repository are in `SHA256SUMS`.
Original PCAP files and proprietary program/firmware binaries are not included.

| Finding | Capture | Setup frame or selector |
|---|---|---|
| out12 USB-direct/custom polarity | `004_route_outputs12.pcap` | 13703: `5800=0`; 41055: `5800=1` |
| out34 routes | `005_route_outputs34.pcap` | 13645: `5900=2`; 54315: `5900=3` |
| loopback routes | `006_route_loopback.pcap` | Values 4, 6, 7, 5 |
| Separate mix gain families | Captures 012 / 007 / 018 | `09/13`, `1D/27`, `31/3B` |
| Mute / solo | Captures 010–011 / 008–009 / 019–020 | `4C–4E` / `4F–51` |
| Three masters | Captures 014 / 016 / 022 | `4500`, `4600`, `4700` |
| Input 3/4 trims, read-only here | Captures 045 / 046 | `4802`, `4803` |
| Unconfirmed stereo activation | Capture 054 | Only `5601=0` captured |
| Copy behavior | Capture 050 | Destination writes; master and USB5/6 not copied |

The CSV has writes only. The B2 layout and the absence of writes when switching
tabs also use before/after state comparisons from the broader collection.
The raw fixture in `crates/mf-core/tests/fixtures/windows-b2.hex` is the final 190-byte B2
state from `057_final_state_verification.pcap`, setup/completion frames
126064/126065. The JSON fixture wraps the same payload in this fork's schema;
zero timestamp and bus/address are deliberate offline metadata placeholders.

## Validation inherited from the reference implementation

- On Linux, a one-step gain change on an already-muted Analog 1 was read back
  and restored in each of the three mixes. All 190 bytes matched the initial
  state after restoration.
- Muted loopback source inclusion and a GUI mute operation were checked and
  restored by full-state comparison.
- Stale-state rejection and invalid selector/channel/value rejection were
  exercised against the reference module.
- On 22 October 2026 the user confirmed audible mute/unmute and level changes
  with a microphone on Analog 1 in out12.

These results belong to the reference implementation. They are not hardware
tests of this Rust fork, nor measurements of audio continuity or latency.
Successful USB completion alone is not proof of correct audible behavior.

## Automated checks in this fork

The test suite decodes the preserved B2 fixture, checks all three model gates
and legacy selector maps, verifies gain/flag/route prediction and opaque-byte
preservation, exercises snapshot and journal failure paths, and checks CLI
parsing/offline operation.

A C probe and Rust test compare validation outcomes and offsets/masks for all
65,536 selectors against 14 representative values, including bounds and silence.
They also check the ioctl number and byte layout. The 712-row evidence test
models the 686 writes within the exposed subset; 26 rows are deliberately
excluded (trim/link, `56`, and USB mono inclusion flags). No test replays USB.

## Hardware checks of this fork

On 28 October 2026, the CLI/kernel sources corresponding to `e3315ad`
were tested with the release Rust CLI and
temporarily loaded module on one MF4. Identical initial reads, an independent
decoder comparison and exact full-state restoration preceded digital loopback
checks of all three mixes. Gain, pan, mute, solo, masters, USB-direct/custom
routes, request rejection and host snapshot failure/restore paths were checked.
The [hardware report](hardware-validation.md) records build hashes, the method,
relative measurements and the limits of the streaming-continuity observation.

MF1/MF2 toggle regression checks require those devices; they were not available
here. Multiple-device, disconnect/rollback, broader topology and power-cycle
tests remain separate hardware work. Neither the automated suite nor testing
one MF4 establishes those behaviors.

## Deliberate boundaries

No MF1/MF2 B2 layout is assumed. No scan of unknown selectors, arbitrary USB
write command, device-storage writer or firmware-update path is added. C2 meters were subsequently calibrated and implemented; see
[the meter report](meters.md). Input trims remain read-only; analog
mono/stereo conversion, `56` activation, LED editing and mix-copy policy are
not implemented. Sample rate and buffers remain the host audio stack's job.
