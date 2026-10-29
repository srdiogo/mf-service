# Installation and operation

Build the locked release, review the files, and stage a package without changing
the current session:

```sh
cargo build --release --locked
make check-distribution
make install DESTDIR=/tmp/mf-service-package
```

The package contains the executable, license and optional user unit. It does
not include or install the kernel module/udev policy. Use the separately reviewed
`mf-cli` package for those prerequisites. Normal operation never needs elevation.
The unit is rendered with the installed executable path (`/usr/bin/mf-service`
for the distribution package). Custom absolute prefixes are supported; `DESTDIR`
is a staging root and is never included in ExecStart.

For development, run `./target/release/mf-service` in a terminal. Runtime access
requires the login session's owned, private `XDG_RUNTIME_DIR`; no shared socket
fallback is offered. Standard XDG state/data locations may be overridden before
startup. Existing private service directories must be owned by the user with
mode 0700. Snapshot/preset/identity files are exclusive mode-0600 files.

Installing unit files does not enable them. After reviewing and installing a
package, optional login startup is a deliberate user choice:

```sh
systemctl --user daemon-reload
systemctl --user enable --now mf-service.service
```

Stop/disable with `systemctl --user disable --now mf-service.service`. Removal
of the executable/unit leaves current device state and user presets/evidence
intact. The Makefile's uninstall target removes only its three package files;
it does not remove user data or change audio configuration.

## Diagnostics

- Missing runtime directory: run inside the owning login session.
- Second instance: connect to or stop the existing service; do not delete a
  live instance's socket/lock. Stale owned sockets are recovered under a lock.
- Missing/incompatible module: install the matching reviewed module through
  normal administrator-managed packaging. There is no raw-USB fallback.
- Access denied: check active-session ACL/group policy for `/dev/minifuse_cmd`.
- Ambiguous device: connect one MF4; the service never chooses one implicitly.
- Stale state/device: discard the old plan, refresh and deliberately re-plan.
- Indeterminate operation: query using the previous client token and key. Read
  current state and retain the service/core records for explicit reconciliation.
  A timeout/restart is never an instruction to repeat a mutation.
- Store full: while stopped, archive explicitly resolved records to an owned
  private location. Never delete unresolved records to unblock a transaction.

Foreground diagnostics and the user's journal are for local troubleshooting;
review/redact host information before sharing. No automatic diagnostic upload,
device reset or preset application occurs during startup/reconnection.

## Distribution preview

The public source repository is <https://github.com/srdiogo/mf-service>. Preview
CI publishes the staged Linux executable/unit/license tree as an artifact. It
is built on the runner's distribution and is not a universal Linux binary.
Build a native package on the target distribution for deployment.

The Arch package recipe is maintained under `packaging/arch/`. It pins a complete
source commit and archive checksum; it does not track a moving branch. Build it
as an ordinary user with `makepkg`, review its contents, then install through
the system package manager. Package installation leaves login startup disabled.
The matching kernel transport is a separate administrator-managed prerequisite.
The upstream v0.2.6 package does not provide these MF4 input/meter extensions.

`make check-distribution` tests installation under a temporary absolute prefix
containing spaces, parses the user unit with systemd, checks installed help and
removes only project-owned files. It does not start a service or alter devices.
A clean-machine build/session check remains distinct from staged validation.
