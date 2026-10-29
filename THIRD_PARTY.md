# Attribution and licenses

This repository combines independently licensed components. Moving them into
one source tree does not change their licenses.

| Component | License | Origin |
|---|---|---|
| `crates/mf-service`, packaging and new project documentation | MIT | Diogo de Medeiros / srdiogo |
| `crates/mf-core` and inherited model/snapshot implementation | MIT | `srdiogo/mf-cli` at `6456df2be9c0a043c16f57a9d279f25085662741`, including earlier work by Pavel Olizko and sleebu |
| `kmod` and the C ABI test probe | GPL-2.0-only | Pavel Olizko, sleebu and subsequent MiniFuse control contributions |

The original MIT notice is preserved in
[`crates/mf-core/LICENSE`](crates/mf-core/LICENSE). The kernel license is
[`kmod/LICENSE`](kmod/LICENSE); source notices are retained. Service and new
material use the root [MIT license](LICENSE). Protocol research and MF4 credit,
including andreanero's prior work, are preserved in
[`docs/mf4/evidence.md`](docs/mf4/evidence.md).

The import is a source-layout change. It preserves typed controls, ioctl layouts,
capability gates and transaction behavior. The original CLI stays in
[its own repository](https://github.com/srdiogo/mf-cli); it is not a dependency of
the backend or desktop applications. The operation backup directory retains its
historical `mf-cli` name for compatibility with existing evidence.
