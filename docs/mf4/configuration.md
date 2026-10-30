# Configuration files and device persistence

The CLI examples below describe the original research interface. They are
optional reference commands; the backend and desktop applications do not
require that executable. See [installation](../INSTALL.md) for this workspace.

`mf-cli config save FILE` reads the current MiniFuse 4 state and creates a new
host-side JSON snapshot. It sends no configuration write. An existing file is
never overwritten. `config show FILE` is entirely offline. `config diff FILE`
reads live state without writing; `--against CURRENT_FILE` compares two files
without opening USB or a device node.

```sh
mf-cli config save studio.json
mf-cli config show studio.json --json
mf-cli config diff studio.json
mf-cli config diff studio.json --against another-state.json
mf-cli config restore studio.json --dry-run
mf-cli config restore studio.json --mix out12
```

## Schema and restore scope

Schema version 1 requires the `minifuse4-b2-190-v1` layout, Arturia vendor ID
and MF4 product ID. It stores a capture timestamp, USB bus/address metadata and
the exact 190-byte state as 380 hexadecimal characters. Bus/address identify
the capture session, not a permanent device identity. Snapshots can be applied
to the sole connected MF4; serial-bound presets are not implemented.

A restore is a new plan of allowlisted parameter writes. It never sends the
raw state block back to the device and never restores opaque header/tail bytes,
input trims, INST, phantom power or LED settings. There is no flash/firmware
image in the snapshot. Applying an unselected mix is also excluded when using
`--mix`. JSON plans report `unrestored_byte_offsets` so omitted differences
remain visible. A snapshot is not a backup of every device setting.

Analog mono/stereo topology must already match. Any changed out34 stereo mask
is refused before writes because the required `56` selector is unvalidated.
Supported analog mono and USB pair membership changes are restored with gains,
mute, solo, masters and routes. Unsupported bits remain at their live values.
The planner preserves stored gain words exactly rather than reconstructing
level/pan and introducing rounding.

Files with another schema, model, layout or invalid hex are rejected. Restore
plans with invalid known writable values are rejected before device writes. Unknown top-level JSON fields are rejected to catch format
mismatches. Snapshot size is bounded to 64 KiB. A missing or incompatible kernel
module does not cause a fallback that detaches the audio driver.

## Backups and operation records

Each nonempty mixer/route/restore operation saves its previous state and a
prepared operation record before invoking the kernel. Both files and their
directory are synced first. Files are created exclusively with mode 0600.
Empty plans send no writes and create no backup files.

The default directory is `$XDG_STATE_HOME/mf-cli`, or
`$HOME/.local/state/mf-cli`. Override it with `--backup-dir DIRECTORY`.

The operation record includes the intended selectors, predicted state and
eventual result. Finalization uses a new synced file followed by rename and
directory sync. A process crash can leave a `prepared` record even if a device
change occurred; inspect live state and the backup before continuing. A failed
final journal update is reported even when the device change was verified.
There is no unattended recovery or automatic replay on startup.

## Hardware storage is separate

Host file durability and a successful B2 readback do not establish that the
device has finished saving to nonvolatile memory. The official MiniFuse 4
manual describes settings retained after closing the application and use
without a computer. Offline investigation of the unmodified official 1.0.1.35
image suggests deferred configuration saving; a nominal scale around ten
seconds depends on clock and execution assumptions and is not a measured or
guaranteed save delay.

This CLI adds no save/flush/reset request, does not write device storage
directly, and does not display a "saved to hardware" indicator. It reports
configuration readback verification only. Power-cycle persistence for this
fork remains an explicit hardware-validation item.

Reference: [Arturia MiniFuse 4 manual, chapter 6](https://dl.arturia.net/products/minifuse-4/manual/minifuse-4_Manual_1_0_0_EN.pdf).
