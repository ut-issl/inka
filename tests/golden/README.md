# Golden vectors

Frozen answers for the layouts that recorded approvals depend on.

An approval record names a `view_hash`. Evaluation counts that record only
while the hash still equals the one recomputed from the current argument, so
the byte layout behind the hash is part of the record's meaning. If the layout
ever shifted — a reordered field, a dropped length prefix, a normalization step
that composed one character differently — every approval anyone had recorded
would stop matching content nobody edited, and the audit trail would be worth
nothing.

The vectors here are what stops that happening. They are replayed by
`crate/inka-core/tests/golden.rs` on every run, and the directory is
**append-only**, enforced in CI: a vector may be added, never edited, renamed,
or removed.

## Layout

```text
hash/v1/                 // view-hash vectors for the v1 layout (`spec.md` → Hashing (v1))
hash/test-evidence-v1/   // test-evidence interface hashes (`test-evidence.md`)
hash/calc-evidence-v1/   // calc-evidence interface hashes (`calc-evidence.md`)
```

Each file is one vector: the inputs a local view is built from, the normal form
its description reduces to, and the resulting hash. Whitespace that a checkout
or an editor could rewrite — CR, trailing spaces, decomposed characters — is
written as a TOML escape rather than as a literal, so a well-meaning formatter
cannot quietly change what is being frozen.

## When a vector fails

Do not update the file. A failure means one of two things:

- the change under test is wrong, and should be fixed; or
- the layout is genuinely moving on, in which case it moves on as a parallel
  v2 with its own domain tags and its own vector directory, leaving v1 and
  every approval recorded against it exactly as they are.
