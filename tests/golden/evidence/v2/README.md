# CI evidence v2 conformance vectors

`contracts.json` freezes independently generated bundle examples and expected
validation/outcome results. The Rust consumer and producer tests consume this
same data. Interface digests use u64 little-endian field framing and the
`inka:ci-interface:v2` domain from the wire specification.

Cases distinguish content identity, required-set identity, CRLF equivalence,
non-passing outcomes, optional metadata, and malformed/inconsistent bundles.
For valid single-target bundles, `status` is `pass`, `fail`, `skip`, `cancelled`
or `missing`. Invalid inputs have `status: null`.

These vectors are append-only. Add new files for new cases or a parallel
version for incompatible contract changes; do not rewrite these answers.
