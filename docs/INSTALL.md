# Installation, operation and removal

The backend is one source workspace and two native packages: `mf-service`
(including the compiled core) and `minifuse-driver-dkms` (control driver and
session access policy). Neither the CLI nor libusb is required. MiniFuse Control
and the Omarchy plugin are optional separate applications.

This is a development preview. The packaged target is Arch Linux x86_64 and the
recorded desktop baseline is Omarchy 4.0.4, Quickshell 0.3.1 and Qt 6.11.2.
MiniFuse 4 has bounded hardware validation. MF1/MF2 may appear in inventory but
receive no inferred MF4 mixer, switch or meter layout. A new kernel, host or
device combination still needs its own acceptance check.

## 1. Prepare the new machine

Use a normal graphical login session. Install development dependencies through
the distribution package manager, including the **headers for the kernel that
will run the driver**. For example, `linux-headers` matches the `linux` kernel;
it is not a replacement for a custom Omarchy kernel's matching header package.
After a kernel update, reboot into the matching installed kernel before testing.

Build requirements are `base-devel`, Rust 1.85+, Git, Python 3.12+ and systemd.
The driver requires DKMS. On Arch, review and install the prerequisites:

```sh
sudo pacman -S --needed base-devel rust git python systemd dkms
```

Install the appropriate kernel-header package separately. Confirm that
`/usr/lib/modules/$(uname -r)/build/Makefile` exists. DKMS also needs the compiler
and linker used by that kernel; Clang-built kernels need the matching LLVM tools.
Do not replace kernel packages or downgrade the desktop to match this preview.

## 2. Build and install the backend

Use a reviewed commit of this repository. From a clean checkout:

```sh
make check
make build
make check-tools
make check-distribution
make arch-source
cd dist/arch
sha256sum -c SHA256SUMS
makepkg --cleanbuild
```

`dist/arch/source-lock.json` records the full commit and package version. Its
source archive contains the core, service, driver, licenses, evidence and build
scripts from that exact commit. Copy this entire directory to the new machine
to build the same source. Cargo registry packages and distribution dependencies
still require network access or a prepared cache; this is not an offline OS
installer or a promise of byte-identical binaries.

Review the generated packages, then install both in one transaction:

```sh
sudo pacman -U ./minifuse-driver-dkms-*.pkg.tar.zst ./mf-service-*.pkg.tar.zst
```

The package manager builds the module through DKMS. Installation does not start
the service, restore settings or enable module loading at boot. Keep
`snd-usb-audio` bound; no driver detach, reset or blacklist is needed.

The old `mf-cli-platform` package owns the same driver and udev paths and
conflicts with `minifuse-driver-dkms`. On a machine using it, review the package
manager's replacement transaction: replacing that combined package also removes
its CLI executable. The new service can instead use its compatible
`minifuse-transport=1` provider until a deliberate driver migration. Never force
overlapping packages or unload a running audio module to complete an upgrade.

## 3. Activate the driver and session access

After successful DKMS installation, explicitly load the control module and
refresh the access policy:

```sh
sudo udevadm control --reload-rules
sudo modprobe minifuse_mod
sudo udevadm trigger --subsystem-match=misc --sysname-match=minifuse_cmd
sudo udevadm settle
```

The active local seat receives access through udev's `uaccess` tag. Check
`/dev/minifuse_cmd` from the owning login session. Do not make it world writable
or run the service as root. If an older module is already loaded, these commands
do not replace it; schedule a reboot into the installed driver instead of
unloading it during playback.

For optional loading on subsequent boots:

```sh
sudo install -m644 /usr/share/doc/minifuse-driver-dkms/minifuse.conf.example /etc/modules-load.d/minifuse.conf
```

Review an existing file before replacing it. This explicit step changes boot
configuration; ordinary package installation does not.

## 4. Start the user service

For foreground operation, run `mf-service`. For deliberate login startup:

```sh
systemctl --user daemon-reload
systemctl --user enable --now mf-service.service
minifuse-doctor
```

Run these as the normal graphical-session user. The service uses the owned
private `$XDG_RUNTIME_DIR/mf-service/control.sock`; it never falls back to a
shared socket. Unit startup does not change device settings. Presets are applied
only by explicit client action, including after reconnect or login.

`minifuse-doctor` requires Python 3.12+. It checks the local installation and
uses only `hello` and `list_devices` through the service. It does not install,
load modules, enable units, apply presets or print device identities/settings.
Use `--json` for a machine-readable report or `--no-service` for offline checks.
Missing optional GUI/Omarchy tools and build headers are reported separately.
A healthy installation report alone is not hardware acceptance.

## 5. Choose a frontend

The pinned desktop source references are in
[`distribution/desktop.lock.json`](../distribution/desktop.lock.json). See
[reproduction and acceptance](REPRODUCING.md) for the exact builds and checks.

- [MiniFuse Control](https://github.com/srdiogo/minifuse-control) is the separate
  full mixer, routing, input and preset application. Its native Arch recipe
  installs the GUI and shared Python package; the desktop agent is optional.
- [Omarchy MiniFuse](https://github.com/srdiogo/omarchy-minifuse) is the compact
  panel and bar widget. After the backend works, follow its normal installation:

```sh
omarchy plugin add https://github.com/srdiogo/omarchy-minifuse.git
omarchy plugin enable srdiogo.minifuse
omarchy bar move srdiogo.minifuse --section right
```

Plugin installation does not install the backend or request privileges. The
full mixer is optional; installing it enables the panel's application launcher.
The physical Arturia-logo button remains unavailable and needs no desktop agent.

## Removal and retained data

Disable the plugin through Omarchy and remove it through the host's plugin
manager. Remove any optional floating-window fragment only if you added it.
Then stop login integration and remove packages deliberately:

```sh
systemctl --user disable --now mf-service.service
sudo pacman -R mf-service minifuse-driver-dkms
systemctl --user daemon-reload
```

Remove dependent GUI/agent packages in the same package-manager transaction if
installed. Remove `/etc/modules-load.d/minifuse.conf` only if it is the file you
created for this setup. DKMS/package removal does not unload the running module;
reboot at a suitable time. No automatic device reset or preset restore occurs.

Package removal retains private operation evidence in
`$XDG_STATE_HOME/mf-service`, historical core backups in `$XDG_STATE_HOME/mf-cli`,
and presets in `$XDG_DATA_HOME/minifuse/presets`, with normal home fallbacks.
Keep unresolved operation records. Archive or delete user data separately only
after reviewing it; do not erase evidence to bypass an indeterminate operation.

## Development staging and diagnostics

`make install DESTDIR=/tmp/mf-service-package` stages the service, doctor,
licenses and optional user unit. It does not install the driver. `PREFIX` is the
installed absolute prefix; `DESTDIR` never appears in the rendered unit.
`make uninstall` removes only those staged project files and leaves user data.

`make check-distribution` verifies staged installation/removal, the user unit,
installed help and SIGTERM cleanup with isolated XDG directories. It does not
enable integration. `make check-driver KDIR=/path/to/kernel/build` builds only;
it never loads the module.

For local diagnostics, inspect `systemctl --user status mf-service.service`,
`journalctl --user -u mf-service.service` and `dkms status`. Review/redact logs
before sharing. A missing module, denied control node, absent private runtime,
multiple MF4s or stale plans have distinct failure paths. Never delete a live
socket or replay an uncertain operation; refresh and reconcile with the original
client token/idempotency key. Unsupported models remain unavailable in the UI.
