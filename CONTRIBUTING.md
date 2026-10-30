# Contributing

Keep protocol/model work in `crates/mf-core`, service coordination and API work
in `crates/mf-service`, Linux transport work in `kmod`, and presentation in
[MiniFuse Control](https://github.com/srdiogo/minifuse-control). The API contract
is [docs/IPC.md](docs/IPC.md). Client fixtures pin a producer commit and SHA-256.

Use purpose-based branches and English source, documentation and commit messages.
Preserve contributor attribution. Explain the behavior changed and validation
performed. Hardware evidence must identify the tested build and distinguish
readback, visual confirmation and measured audio. Share synthetic fixtures or
anonymous measurements rather than private snapshots, serials or microphone PCM.

Run `make check`, `make build` and `make check-distribution`. The distribution
check stages/removes files and verifies SIGTERM cleanup of an isolated process
with private temporary XDG directories; it never enables session integration. Hardware validation is separate and must preserve the full initial
state. MF1/MF2 support requires model-specific evidence rather than MF4 guesses.

Firmware modification, effects, raw request interfaces and automatic preset
replay are outside this project's scope. Installing a unit never enables it.

Run `make check-driver KDIR=/path/to/kernel/build` for driver changes. This
builds without loading the module. Use `make arch-source` to export a clean,
pinned package source kit; see [packaging](packaging/arch/README.md).

The standalone installation entrypoint belongs under `distribution/`, separate
from the service and plugin runtime. Root commands are fixed package/driver
operations; all session/configuration work remains unprivileged. Test through
fake hosts and temporary directories, never by enabling integration in CI.
After committing, use `tools/prepare_installation_kit.py` with `--base-kit /path/to/verified-kit`
and `--output /path/to/new-kit` to package the current installer alongside the pinned
native artifacts. The base kit remains unchanged. The generated manifest and
build record identify the installer revision separately from the runtime source.
