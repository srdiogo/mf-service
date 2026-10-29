# MiniFuse Linux installer

Install the complete MiniFuse backend, standalone GUI and Omarchy panel from
this verified kit. The core is compiled into the one user-session service.
No CLI or Rust toolchain is needed to run the native packages.

## Quick start

Extract the kit on the new machine and run, **as your normal graphical user**:

```sh
./install.sh
```

The installer first checks all bundled checksums, package identities, kernel
headers, Python version and session access. It displays the exact plan before
requesting confirmation. Only system package/module/access setup uses sudo;
service activation and desktop configuration stay in the owning user session.
Do not launch the whole script with sudo.

The default installs the backend and GUI, and adds the plugin when Omarchy is
already present. On Omarchy it also configures the full GUI as a floating window.
Login startup and driver loading at boot are enabled by the displayed plan.

To verify everything without making changes:

```sh
./install.sh --dry-run
```

For the full Omarchy setup, accepting the plan and package prompts:

```sh
./install.sh --profile full --yes
```

Sudo may still require authentication. The installer does not install Omarchy
itself, perform a system upgrade, downgrade packages or reboot the machine.
If Python is absent, the shell entrypoint offers to bootstrap it through pacman.
Inspection mode never bootstraps prerequisites.

## What is installed

1. Distribution dependencies and, when identifiable and version-matched, the
   running kernel's header package. Clang kernels receive their LLVM tools.
2. `minifuse-driver-dkms` and `mf-service` from this kit. DKMS builds the control
   driver; udev gives the active local session access without world-writable
   permissions. Normal `snd-usb-audio` stays bound.
3. The optional shared Python package and MiniFuse Control GUI. Native GUI
   packages are bound to the Python minor version used for their site-packages
   directory; this preview requires Python 3.14. Rebuild for another supported
   interpreter rather than forcing dependency checks.
4. The optional Omarchy plugin, cloned from the verified local Git bundle and
   checked out at its pinned commit. It is enabled in the right bar section.
5. The optional floating/centered GUI policy, using the rule packaged by the
   plugin. The main Lua config is backed up. Reload/configuration errors restore
   the original files and retain the backups.
6. The optional user unit and boot loading policy, then a read-only diagnostic.
   Physical Arturia-logo events are unavailable; the desktop agent is not enabled.

GUI and plugin remain separate projects. The script runs no mixer writes,
firmware changes, effect controls, preset restoration or test tones. A device can
be absent during installation; the final diagnostic reports that separately.
An already running service is not forcibly restarted, and a loaded older module
is never unloaded. Installed replacements take effect on a suitable restart or
reboot; the diagnostic helps identify missing capabilities.

## Profiles and options

| Option | Behavior |
|---|---|
| `--profile auto` | Backend and GUI; plugin when Omarchy is available (default) |
| `--profile full` | Backend, GUI and plugin; requires an existing Omarchy host |
| `--profile backend` | Driver and service only |
| `--profile gui` | Backend and GUI without installing the panel |
| `--profile omarchy` | Backend and panel without the full GUI |
| `--dry-run` / `--check` | Read-only verification and complete plan |
| `--yes` | Accept plan/package prompts; sudo authentication is separate |
| `--headers PACKAGE` | Select the matching header package if detection is unavailable |
| `--no-startup` | Preserve login/boot policy; start the current session only |
| `--no-start` | Install files without explicitly loading, starting or enabling components |
| `--no-floating` | Preserve compositor configuration |
| `--keep-driver` | Keep an installed compatible `minifuse-transport=1` provider |
| `--replace-legacy-driver` | Review replacement of `mf-cli-platform` interactively in pacman |
| `--update-plugin` | Back up and replace a different/customized plugin checkout |
| `--desktop-agent` | Install the optional agent package without enabling activation |

`--keep-driver` and `--replace-legacy-driver` are mutually exclusive. Replacing
the old combined `mf-cli-platform` package also removes its CLI executable, so
it cannot be combined with `--yes`. The retained driver must already provide
transport version 1 and be installed for the running kernel.

Custom module boot policy and custom floating rules are not overwritten. Use
`--no-startup` or `--no-floating` to retain them. An existing different plugin
requires `--update-plugin`; the old directory moves to a private backup outside
the host's plugin registry, avoiding duplicate plugin IDs. Repeating the same
installation reuses a clean matching plugin and does not duplicate window rules.
Files copied into a previously enabled plugin may be noticed by the host's
normal file watcher, including with `--no-start`.

## Supported environment and kernel headers

The native backend targets Arch Linux x86_64. The GUI was checked with Qt 6.11.2
and 6.12.0; the pinned Omarchy host reference is 4.0.4, Quickshell 0.3.1 and Qt
6.11.2. Other host versions need their own acceptance test. MF4 is the validated
mixer model; MF1/MF2 do not inherit its control layout.

The header package is inferred from the running kernel's `pkgbase`. Its
repository version must match the installed kernel package. If matching headers
are unavailable, setup stops before installing the backend. Update/reboot or
install the matching headers separately, then rerun. No kernel replacement,
repository refresh or full/partial system-upgrade workaround is performed.
A custom kernel without package identity requires manually installed matching
headers. Fresh binary installation needs compiler/DKMS dependencies, not Rust.

## Records, recovery and removal

Local progress records and backups are under
`$XDG_STATE_HOME/minifuse-installer` (default `~/.local/state/minifuse-installer`),
owned by the user. Nothing is uploaded. If a stage fails, completed stages are
reported and retained. Resolve the stated cause and rerun; package installation
is not presented as an atomic transaction spanning desktop configuration.

The final diagnostic sends only read requests. A successful build, staged test
or existing-session check is not fresh-machine hardware acceptance. Use
`docs/REPRODUCING.md` in the extracted kit for controlled validation and
`docs/INSTALL.md` for manual operation/removal.

To remove the installation, disable/remove the panel through Omarchy, stop and
disable `mf-service.service`, and remove installed native packages through
pacman. Remove the specific floating-rule require/file or restore the backed-up
configuration deliberately; do not restore an old whole config over newer user
edits. Remove only the module boot-policy file created by this setup. Driver
removal does not unload the active module; reboot at a suitable time.

Presets, microphone power, mixer state and unresolved operation records are
never removed by this installer. Project source, attribution and licenses are
included in the kit. SHA256SUMS checks integrity against this reviewed profile;
it is not a signature for an independently obtained untrusted kit.
