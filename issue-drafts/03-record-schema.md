# record: approval record schema (v1) as code — serde structs + canonical signing bytes

- **Labels:** enhancement
- **Milestone:** M0 (spec freeze — reinterpreted as code-first)
- **Depends on:** nothing (can proceed in parallel with 01/02)

## Motivation

Same code-as-spec treatment for the second frozen item from the roadmap
(`spec/record.md`). The frozen artifacts are the serde struct definitions, the
canonical-signing-bytes function, and their golden vectors. Signatures themselves are
not implemented until M7, but the `sig` field's slot and the exact byte string that
will be signed are frozen **now**, so adding signatures later cannot break the format.

## Design decisions to settle in this issue

Recorded as rustdoc with a one-line rationale each:

1. **Field set**: `target, view_hash, label, comment, approver, key_id, timestamp,
   phase, until, policy_hash, sig, v`. Which fields are optional (`until`, `comment`
   for approve) and what the `v` version field means.
2. **Canonical signing bytes**: the compact JSON serialization with keys sorted,
   excluding `sig`. This byte string is what gets signed in M7 and what
   `policy_hash` / `phase` / `until` protection relies on (concept §9.1).
3. **jsonl conventions**: append-only; one self-contained attestation per line;
   files may be split per user (`approvals/<user>.jsonl`) to avoid merge conflicts.
4. **Error handling on read**: malformed lines fail loudly with file/line position —
   never silently skipped (a skipped line could hide a decline).

## Tasks

- [ ] serde structs + jsonl read/write.
- [ ] `canonical_signing_bytes(record) -> Vec<u8>` as a pure function.
- [ ] Golden vectors for the canonical bytes (record → exact byte string), committed
    to `tests/golden/`.
- [ ] Rustdoc for all decisions above.

## Acceptance criteria

- [ ] Round-trip test: parse → serialize → parse is identity.
- [ ] Golden vectors for canonical signing bytes pass.
- [ ] A record with an unknown/missing field is rejected with a clear error.

## References

- inka-concept.md §9.1, §9.2
- inka-implementation-roadmap.md §2 (spec/record.md, adapted to code-first)