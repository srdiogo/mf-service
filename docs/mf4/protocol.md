# MiniFuse 4 control protocol

The CLI examples below describe the original research interface. They are
optional reference commands; the backend and desktop applications do not
require that executable. See [installation](../INSTALL.md) for this workspace.

This reference describes configuration controls observed on one MiniFuse 4
(`1c75:af70`, displayed firmware 1.0.1), using MiniFuse Control Center
1.4.2.1381 and Windows driver 1.4.0.0. It does not describe the MiniFuse 1/2
state layout. See [evidence and attribution](evidence.md) for sources and limits.

## USB requests

| Operation | bmRequestType | bRequest | wValue | wIndex | Length |
|---|---:|---:|---|---:|---:|
| Read peaks | `0xA1` | `0xC2` | `0` | `0` | 64 bytes |
| Read state | `0xA1` | `0xB2` | `0` | `0` | 190 bytes |
| Set one parameter | `0x21` | `0x22` | `(feature << 8) \| channel_or_pair` | `0` | 2 bytes |

These are USB Class/Interface requests with proprietary semantics. Each payload
word is little-endian. Levels are signed int16 in 1/256 dB; `0x8000` means
silence. The kernel module keeps `snd-usb-audio` bound. This implementation has
no userspace USB fallback for the new commands.

## Per-mix selector families

The table contains hexadecimal feature bytes. Append the source channel or pair
index as the low byte; master and route require low byte zero.

| Control | out12 | out34 | loopback |
|---|---:|---:|---:|
| Left gain | `09` | `1D` | `31` |
| Right gain | `13` | `27` | `3B` |
| Master | `45` | `46` | `47` |
| Mute | `4C` | `4D` | `4E` |
| Solo | `4F` | `50` | `51` |
| Mono source inclusion/display | `52` | `53` | `54` |
| Stereo pair inclusion/display | `55` | `56` (blocked) | `57` |
| Route | `58` | `59` | `5A` |

There is no observed global mix-selection write. The `1D/27/46` family belongs
to out34; changing application tabs did not issue configuration writes.

Channels 0–3 are Analog 1–4; 4–9 are USB 1–6. Only channels 0–7 are exposed for
out12/out34, and 0–9 for loopback. Pair indices are 0=Analog1/2, 1=Analog3/4,
2=USB1/2, 3=USB3/4, 4=USB5/6. USB5/6 is available only in loopback.

Channel gains accept signed words through +12 dB, including the lower bus gains
needed near pan extremes. The user-facing source-level range is -60..+12 dB
or `-inf`. Master range is -64..0 dB or `-inf`. Flags accept exactly 0 or 1.

Only analog mono inclusion is exposed through `52/53/54`. The captured USB mono
flag writes are preserved as evidence, but their interaction with stereo source
creation is not implemented. The CLI exposes USB pair inclusion only for out12
and loopback. Analog mono/stereo conversion and all `56` writes are blocked by
the application. The kernel also rejects every `56` write. Only `5601=0`, not
activation with 1, was captured.

Removing a source through the CLI additionally sets mute and clears solo for
its affected channels. Inclusion/display flags are not advertised as calibrated
audio gates; their isolated audible effect has not been established.

## Route values

| Selector | Value | Meaning |
|---|---:|---|
| `5800` | 0 / 1 | USB1/2 direct / custom mix |
| `5900` | 2 / 3 | USB3/4 direct / custom mix |
| `5A00` | 4 / 5 / 6 / 7 | USB5/6 / own mix / out12 / out34 |

The out12 polarity corrects the September community notes: capture
`004_route_outputs12.pcap`, setup frames 13703 and 41055, shows `5800=0` for
USB direct and `5800=1` for custom mix. Other combinations are rejected.

## B2 layout

Offsets are decimal, from zero; all listed words are little-endian.

| Field | out12 | out34 | loopback |
|---|---:|---:|---:|
| Ten left gain words | 18 | 58 | 98 |
| Ten right gain words | 38 | 78 | 118 |
| Master | 138 | 140 | 142 |
| Mute mask | 152 | 154 | 156 |
| Solo mask | 158 | 160 | 162 |
| Mono inclusion/display mask | 164 | 166 | 168 |
| Stereo inclusion/display mask | 170 | 172 | 174 |
| Route | 176 | 178 | 180 |

The INST mask is the word at byte 0 (bit 0 = INST1, bit 1 = INST2).
The word at byte 6, bit 0, is shared phantom power for inputs 1–2.
All other header bits and bytes 182–189 are preserved. These input fields are
MF4-specific; see [physical validation](input-switches.md).

Typed input writes are `0000` (INST1), `0001` (INST2) and `0300`
(shared phantom power), each accepting exactly 0 or 1. Input switches are never
replayed by mixer snapshot restoration.
Bytes 144–151 are the trim area; Input 3/4 are at 148/150. Trims are decoded
read-only. Ten stored gains per bank do not establish support for USB5/6 in
the output mixes.

The captures identify Input 3/4 trim writes at `4802/4803`, and trim link at
`0600`. These are deliberately excluded from the writer pending complete
readback/link semantics and Linux validation. Do not generalize to `4800/4801`.

## Level and pan

There is no separate captured pan selector. Source level and pan are derived
from the two stored bus gains using an equal-power model. A centered source
at 0 dB produced `FE FC` on each bus (-3.0078125 dB); the extremes produced
0 dB on one bus and silence on the other. The implementation uses truncation
for calculated bus gains. Exact official-app quantization across every value
is not established. Changing mute or solo leaves both gain words untouched.

## Kernel ABI

`/dev/minifuse_cmd` keeps the existing ASCII write ABI for known model-specific
toggles. It additionally exposes one 206-byte status record and two ioctls:

- Capabilities: `_IOR('M', 0x42, struct mf4_capabilities)`, 16 bytes.
  `MF4C`, LE16 version=1, LE16 length=16, LE32 flags, LE32 reserved=0.
  Flag bit 0 enables verified input-switch transactions. This query requires
  read access and performs no USB transfer. `ENOTTY` means an older module
  with no additional features; input writes must remain disabled.


- Status: `MF4S`, LE16 version=1, vendor, product, bus, address, length=190,
  followed by the 190-byte B2 payload. Reads are MF4-only and refuse multiple MF4s.
- Apply: `_IOWR('M', 0x41, struct mf4_transaction)`, 1032 bytes. Version, bus,
  address and count precede the expected 190-byte state; up to 160 selector/value
  pairs follow; then returned state, completed count and flags. See
  [`mf4_protocol.h`](../../kmod/mf4_protocol.h) for the authoritative layout.
- Flags: 1=rollback attempted, 2=rollback verified, 4=returned state valid.

160 commands cover one complete supported mixer restore without splitting it
into independent transactions. Unused entries are zeroed. The kernel validates
the entire plan, rejects duplicate selectors, validates every original value for
rollback, compares device identity and complete expected state, and serializes
reads, mixer transactions and legacy writes under one mutex.

Short USB transfers fail. Success requires a complete matching B2 readback.
On failure after a write attempt, rollback includes the failed selector, since
a transport error cannot prove that the device ignored it. Rollback is attempted
in reverse order and separately verified. These requests are not atomic; the
device can briefly pass through intermediate settings. External control clients
are not covered by the module's mutex. Concurrent changes may cause rejection
or readback failure; rollback cannot be guaranteed after disconnection.

## Signal peak readback

C2 holds 16 LE32 magnitudes: Analog 1–4, USB 1–6, and stereo out12/out34/loopback
mix outputs. Full scale is `2^31`; zero is silence. Conversion uses
`20 * log10(value / 2^31)` dBFS. Mix peaks are after master gain and before
destination routing; they remain custom-mix peaks when USB direct is selected.

Module capability flag bit 1 advertises `_IOR('M', 0x43, struct mf4_meter_record)`.
The packed 80-byte record has the same 16-byte identity header as status, with
magic `MF4M`, version 1 and length 64, followed by C2. Read access is required.
The module fixes every USB field, uses the shared control mutex, rejects multiple
MF4 devices and leaves audio-driver bindings in place. No caller-supplied USB
request, selector or firmware operation is accepted. Calibration and untested
cases are recorded in [the meter report](meters.md).
