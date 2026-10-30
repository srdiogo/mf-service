# MiniFuse 4 hardware validation

## Scope and tested build

On 28 October 2026, release Rust CLI and kernel-module artifacts were
exercised on one MiniFuse 4 (`1c75:af70`). Their CLI/kernel sources
correspond to revision `e3315ad3f3df9d00125bb2d44c374b3757210622`.
This is validation of this fork, separate from the earlier
Python reference implementation. It does not establish compatibility with
other MiniFuse models or every MF4 firmware revision.

Selected numeric results and completion flags are preserved in
[`hardware-validation-2026-10-28.json`](hardware-validation-2026-10-28.json).
It contains no captured PCM, serial numbers, host identifiers or user snapshots.

| Component | Tested version or SHA-256 |
|---|---|
| Rust compiler | 1.85.1 |
| Kernel | 7.2.5-3-omarchy |
| PipeWire | 1.6.8 |
| Release CLI | `b870d8d9eb66b8bf3f44eb9bbe0c9e4b261ca7f13701b3ff450df95db63b527d` |
| Kernel module | `41e6c6c46620be5b2ef74cdece950b871fa3f43ebbf019658be879dbb3f42418` |

The module was loaded temporarily, without installing a udev rule, DKMS
package or startup service. The existing `snd-usb-audio` bindings were retained.
The connected setup had a phantom-powered microphone on Analog 1 and speakers
on outputs 1–2. Phantom power was already enabled and remained enabled.
The device's full firmware build was not queried in this session.

## State and transport checks

Three initial B2 reads were identical. The Rust decoder's gains, masters,
routes, mute and solo fields agreed with the independent reference decoder.
Each completed mixer test phase restored the original **entire 190-byte B2
payload**, including fields outside the writable subset.

- Mixer, route and restore previews worked with a read-only device node and
  did not change device state.
- A one-step gain change of 1/256 dB on an already-muted Analog 1 produced only
  the predicted changes in each of the three mixes, followed by exact restore.
- A valid empty ioctl completed zero USB writes and returned valid readback.
- The following invalid requests were rejected with unchanged full state:

| Request | Kernel error |
|---|---|
| Apply on a read-only file descriptor | `EACCES` |
| Unvalidated `56` stereo activation | `EOPNOTSUPP` |
| Unsupported channel, invalid route, duplicate selector or excess count | `EINVAL` |
| Master above 0 dB | `ERANGE` |
| Stale USB device address | `ESTALE` |
| Stale expected B2 state | `EAGAIN` |

Snapshot saving refused to overwrite an existing file. An unusable backup
directory prevented the operation before any device change. CLI channel,
topology and value restrictions were also exercised. A snapshot restore of 20
changed gain words completed as one 20-command transaction with verified
readback and restored all 190 bytes. The changed sources were muted and were
not in the active loopback route.

## Audio behavior

The measurements used synthetic stereo PCM at 48 kHz, with a 1 kHz carrier on
the left and 1.6 kHz on the right, short ramps and a peak of 0.01 full scale.
Each selected USB pair was routed through its custom mix and observed at the
hardware loopback capture. For out12/out34, loopback followed the corresponding
output mix. Loopback's own mix was measured directly. These are digital path
measurements; they do not calibrate the analog outputs or microphone input.

Relative carrier amplitudes were measured over a steady 0.5-second window.
Gain comparisons used a tolerance of 0.15 dB; suppression checks required the
carrier to be at least 60 dB below its reference. No deeper noise-floor or
dynamic-range claim is made when a captured carrier was zero.

| Mix | Source set to −6 dB | Center pan, left | Center pan, right |
|---|---:|---:|---:|
| out12 | −5.999995 dB | −3.007807 dB | −3.007806 dB |
| out34 | −5.999958 dB | −3.007823 dB | −3.007805 dB |
| loopback | −5.999958 dB | −3.007823 dB | −3.007805 dB |

All three mixes passed source gain, center/extreme pan, mute, solo and master
attenuation checks. Unselected carriers retained their expected levels.
The center-pan result agrees with the encoded −770/256 dB bus gains.

Routing tests confirmed that USB direct bypasses custom-mix mute and master
attenuation. Returning to the custom route restored their effect:

| Destination | USB direct | Custom mix |
|---|---|---|
| out12 (`5800`) | `0` | `1` |
| out34 (`5900`) | `2` | `3` |
| loopback (`5a00`) | `4` | `5` |

Loopback following out12 (`6`) and out34 (`7`) was exercised during these
measurements. USB pair removal/addition on out12 and loopback changed the
expected state and removed/restored both carriers. Removal also mutes the pair;
this test does **not** isolate an audio-gating effect of the inclusion flag.
Muted Analog 3 membership on out34 was checked by readback only. No `56` write
was sent.

An initial out12 measurement affected by unrelated application playback was
discarded. The successful out12 run was performed with that playback paused.
Host stream levels were kept fixed within each comparison; absolute levels
across different USB pairs are not compared.

### Controls during streaming

During an eight-second loopback test, one carrier was muted at approximately
2.04 seconds and unmuted at 4.07 seconds. Analysis found 101 muted windows on
the controlled carrier and no missing reference-carrier windows among 374
analyzed 20 ms windows. ALSA reported the stream running, and the USB audio
driver bindings were unchanged at both control events.

This is a bounded continuity observation, not a formal latency or dropout
benchmark. Sub-window interruptions, other loads and other sample rates were
not evaluated.

### Microphone and existing toggles

The existing `48v on` command was idempotent against the initial full state;
phantom power was not cycled off with the microphone attached. INST1 and INST2
writes changed only header byte zero, with the mixer bytes unchanged. Both
inputs were restored to their initial line/microphone mode. Visual LED
confirmation was requested but had not been received when this report was
written, so these are control/readback results only.

Analog 1 was then monitored through out12 for 60 seconds at source level
−18 dB, center pan and master −12 dB, with loopback following out12. Only
anonymous one-second RMS/peak statistics were retained; voice PCM was processed
in memory and discarded. The maximum captured peak was approximately −80.8
dBFS, and the last two one-second windows after mute contained digital zeros.
The low signal level and absence of a listening confirmation do not establish
successful spoken-voice monitoring through the physical speakers. That check
remains open.

## Restoration and cleanup

After the final test, all 190 bytes matched the initial payload, including the
original INST and phantom-power state. All original USB driver bindings were
unchanged. The temporary module was unloaded and its device node disappeared.
No host microphone mute, persistent audio routing or service configuration was
changed.

## Repeating the checks

1. Build the documented CLI/module revision and load the matching module. Keep
   speaker volume low, keep the microphone away from speakers, pause unrelated
   playback and record the existing LED state. Use a separate backup directory.
2. Save a new snapshot with `mf-cli config save baseline.json`; read status
   repeatedly and compare the complete payload. Preview changes before applying.
3. Begin with an already-muted source. Apply a one-step gain change separately
   in each mix, verify predicted readback, restore the snapshot and compare all
   bytes before proceeding.
4. For audio checks, use fixed-level stereo tones on the relevant USB pair.
   Select the custom mix and observe it through the loopback route. Test one
   control at a time against a steady reference; compare direct/custom routes
   with custom mute/master attenuation active. Restore between mixes.
5. Treat a source-membership operation as a combined state change, including
   mute/solo. Do not infer an isolated flag's audio behavior from that operation.
6. Restore mapped fields from the snapshot, restore any legacy toggles
   separately, then require exact full-state equality. Unload a temporary module
   after all clients close it. Do not power-cycle the interface as a substitute
   for verified restoration.

## Remaining hardware coverage

The session did not test MF1/MF2 hardware, multiple connected interfaces,
forced USB errors/disconnection during rollback, power-cycle persistence,
all supported sample rates or every source/topology combination. Rejection of
an invalid request is not evidence that recovery after a partial USB failure
works on hardware.

Configuration readback and durable host files do not prove nonvolatile device
saving. See [configuration and persistence](configuration.md).
