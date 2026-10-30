# Arch source and native packages

This repository owns the complete backend source. `make arch-source` exports a
clean Git revision to `dist/arch`: deterministic source archive, generated
`PKGBUILD`, source lock and checksums. The recipe template is `PKGBUILD.in`.
No source is fetched from the CLI repository and no moving branch is embedded
in the generated recipe. The archive and recipe agree on one full commit.

Build as an ordinary user with the dependencies in [INSTALL.md](../../docs/INSTALL.md):

```sh
make arch-source
cd dist/arch
sha256sum -c SHA256SUMS
makepkg --cleanbuild
```

The split packages are:

| Package | Contents | Activation |
|---|---|---|
| `mf-service` | Service with `mf-core` compiled in, optional user unit, read-only doctor and MIT notices | Explicit foreground run or user-unit enablement |
| `minifuse-driver-dkms` | GPL driver source, DKMS metadata, active-seat udev access and boot-policy example | DKMS builds on package installation; module load/boot policy are explicit |

The service depends on `minifuse-transport=1`, provided by the driver package.
An existing compatible `mf-cli-platform` also provides it; the two driver
packages conflict because they own the same paths. Review replacement/removal
when migrating an existing machine. No firmware, effects or device-storage
commands are packaged.

The build never loads a module or enables a unit. `make check-driver` builds
against selected headers separately. CI tests the current commit's source kit,
not a recipe accidentally pinned to an earlier implementation. It uploads both
native packages, the complete source kit and checksums as preview artifacts.
This is a coordinated source profile, not an offline distribution mirror or a
guarantee of bit-identical binaries across compilers/distributions.
