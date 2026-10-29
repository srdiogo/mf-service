# MiniFuse 4 signal meters

## Read-only transport

On 29 October 2026, the fixed `A1/C2`, value/index 0, 64-byte request was tested
on one MiniFuse 4 (`1c75:af70`). This is the recurring read observed in official
Control Center captures. The temporary kernel module had SHA-256
`c914b49ec380f8e8ebb40e9577ab663ecc7d9a6e04abbf78a02d20596b8a4f2a`
and was built for kernel `7.2.5-3-omarchy`. The query is serialized with existing
controls and does not detach `snd-usb-audio`. Its first bounded sample collection
left all 190 configuration bytes unchanged.

## Channel and scale calibration

At 48 kHz, isolated 1 kHz float PCM was sent through each of the six USB channels.
After unrelated playback was paused, a peak amplitude of 0.0001 produced median
C2 source magnitudes of 214748 for all six channels. This equals -80.000015 dBFS
when converted with `20 * log10(value / 2^31)`. A separate USB3–6 run at amplitude
0.0002 produced 429497, or -73.979395 dBFS. An earlier USB1–2 run contaminated by
unrelated playback was excluded from scale calibration.

The final run exercised all six mix output channels with masters at 0, -6 and
-12 dB. Loopback USB5/6 were temporarily unmuted and soloed through ordinary
verified service operations. All six channels returned the same medians:

| Master | USB source peak | Mix peak | Mix dBFS |
|---:|---:|---:|---:|
| 0 dB | 214748 | 214752 | -79.999853 |
| -6 dB | 214748 | 107631 | -85.999827 |
| -12 dB | 214748 | 53944 | -91.999658 |

All 18 mix measurements agreed within 0.001 dB with the expected values.
The original loopback destination remained USB direct during these checks;
therefore its C2 pair measures the custom mix before route selection. Complete
190-byte configuration equality was confirmed after the temporary controls
were restored. The INST/phantom state remained unchanged.

A separate two-second Analog 1 capture compared the device meter with the
microphone's signed 32-bit PCM. Both maxima were exactly 2039552. Only anonymous
peak statistics were retained; microphone PCM was processed in memory and
not saved. The first four C2 slots correspond to analog inputs, followed by six
USB slots and six mix-output slots. Active signal injection into Analog 2–4
was not performed.

## Limits

These tests cover this unit at 48 kHz. They do not calibrate analog voltage,
noise floor, clipping indicators, peak-hold timing, every sample rate or every
firmware revision. Values represent device peak magnitudes, not loudness or
RMS. There is no claim about physical speaker levels when routing bypasses a
mix. MF1/MF2 layouts are not inferred. Meters do not belong in configuration
snapshots or operation journals; loss of fresh data must clear their display.
