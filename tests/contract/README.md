# Canonical API v1 fixtures

`v1.json` is generated from the real coordinator and the deterministic test
backend in `tests/coordinator.rs`. It covers negotiation, state, planning,
durable admission, verified completion and a structured error. It is never a
production device or runtime fallback. Consumers must pin this repository's
revision and the fixture SHA-256; do not hand-edit client copies or define a
second wire specification.

Opaque IDs/paths are replaced with illustrative placeholders. Derived floating
values are rounded to six decimals for cross-platform fixture comparisons;
production responses retain the core's precision. Tests compare the rendered
fixture exactly. Deliberate regeneration uses:

```sh
MF_SERVICE_UPDATE_FIXTURES=1 cargo test --test coordinator canonical_wire_fixture
```

`tests/fixtures/windows-b2.hex` is the existing sanitized state fixture from
`mf-cli` commit `4775fd23e0bff5b28bbd138e4108dc8e519a0021`. Its protocol provenance
and upstream MF4 contributor credits are recorded in that repository's
`docs/mf4/evidence.md`. No microphone samples, serial numbers or raw capture
collections are included here.
