# MiniFuse 4 integration validation

On 29 October 2026, service revision
`8373de87cc5aa768ab83666f5f662021800e0c4e` and GUI revision
`5bdbe44ab5b87e0bd0d96513743f0adf2847b2d2` were exercised on one MiniFuse 4.
The core and module protocol are versioned in this workspace. Historical
hardware observations remain tied to the source revisions cited below.
The module was loaded temporarily; no startup service or persistent audio
configuration was installed.

## Input controls

INST1, INST2 and shared phantom power were clicked in the GUI and checked
against operator-confirmed physical LEDs. Independent B2 reads found only the
expected input bit changed. Restoring both INST switches off and +48 V on
restored all 190 bytes. Physical-button readback was also checked separately.
See [the core input report](https://github.com/srdiogo/mf-cli/blob/004d6156edce1a45d2a888fbb0f00bc76b7d6a41/docs/mf4/input-switches.md).

## Signal meters

The core calibration covered six USB sources and six post-master mix channels
at 48 kHz, including 18 isolated master measurements. A microphone input-1
peak matched the in-memory PCM peak exactly. Calibration, source hashes and
limits are recorded in [the meter report](https://github.com/srdiogo/mf-cli/blob/936962fcef9320fbf0e0991bd095d9c6c9370dc4/docs/mf4/meters.md).

A live subscription received 25 valid, consecutively sequenced meter events at
an observed mean 19.83 Hz. This is a bounded observation, not a timing guarantee.
The complete B2 state and operation-file count were unchanged. A real Qt client
connected to the hardware-backed service and rendered eight source bars and
two master bars for the selected output mix. All were marked available; the
Analog 1 bar followed a nonzero native peak. The configuration revision stayed
unchanged during this observation. The visible Wayland application opened
without runtime-log errors.

Offline validation passed 24 service tests and 19 GUI tests under both Basic
and Fusion styles, including socket behavior, meter expiry and preservation of
edits. Core tests and the independent module build are tracked in the core PRs.
Active analog signal injection into inputs 2–4, other sample rates, logo-button
events, forced rollback failures and nonvolatile saving remain outside this
validation. No microphone PCM, full private state, device serial or host ID is
included in this report.
