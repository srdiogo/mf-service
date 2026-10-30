# Installation and reboot validation

On 29 October 2026, the operator confirmed that the delivered complete installer
worked immediately, without requiring a reboot, and continued to work after a
subsequent reboot. This records operator-observed installation and post-reboot
operation in the tested environment.

## Delivered kit reference

| Item | Reference |
|---|---|
| Archive | `minifuse-linux-installer-2026-10-29.tar.gz` |
| Archive SHA-256 | `a2f98a053369e1a1ada2bc4b9aa81a90b7967ab1694a91ac4df7f9ed68a9fefd` |
| Installer source | `656862e1537bdae6f7068f00cf50e6f54262be72` |
| Backend packages | `0.2.0.r16.gcb26ef0-1` |
| GUI packages | `0.1.0.r19.g2481510-1` |
| Plugin source | `3cc6116680a50794c933f930b4e498623c01c0cc` |

These references identify the delivered artifact. Installed versions and the
machine's initial setup were not independently collected with this report, so
it does not classify the machine as a clean installation or an upgrade.

The prior automated checks covered 19 packaging/diagnostic/installer tests,
including isolated simulated installation, privilege boundaries, reruns,
backups and configuration-error recovery. The delivered archive was extracted
and every checksum verified. A read-only dry run on the existing development
host completed with unchanged desktop-file checksums. The full CI result is
[recorded here](https://github.com/srdiogo/mf-service/actions/runs/40930560277).

This operator confirmation adds real installation/reboot evidence to those
software checks. Model-specific controls, other kernels/desktop versions,
controlled disconnect recovery and MF1/MF2 support retain their existing
validation boundaries. See [the reproduction checklist](REPRODUCING.md) and
[the MF4 hardware report](MF4_VALIDATION.md).
