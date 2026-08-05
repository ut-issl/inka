# hash: golden test vectors — independently verified, then frozen

- **Labels:** enhancement
- **Milestone:** M0 (spec freeze — reinterpreted as code-first)
- **Depends on:** 01 (local-view hashing v1)

## Motivation

With code-as-spec, the golden vectors *are* the frozen specification: the
implementation may be rewritten at any time, but these vectors must never change
(a change means a `v2`). The one risk of generating vectors from the implementation
is that an implementation quirk (e.g. accidental NFD instead of NFC, serde map
ordering) silently becomes the spec. Independent verification of a small sample
closes that hole at minimal cost.

**Note on `bind` vectors:** the framing of the bind hash (three sorted, length-
prefixed components) is frozen in v1, but the real content of "source statement" and
"aggregated assumption set" only gets pinned down by the veriq integration (M8).
Bind vectors here therefore use synthetic strings — they freeze the framing, not the
veriq-side content.

## Tasks

- [ ] Generate 10+ vectors from the reference implementation, covering:
  - all three kinds: `structure` / `bind` / `fiat`
  - with and without deadline annotations (`until:` / `promote:`)
  - pairs that differ only in child order and **must** produce the same hash
  - a pair differing by a single character that must produce different hashes
  - non-ASCII descriptions (Japanese text, combining characters) to exercise NFC
- [ ] Independently re-derive **2–3 vectors without the Rust implementation**
  (hand calculation or a throwaway Python script) and check they match.
  Keep the throwaway script in the PR description or a comment, not in the repo.
- [ ] Commit vectors to `tests/golden/` and wire them into the test suite.
- [ ] Treat `tests/golden/` as append-only from now on. (A CI guard that fails any PR
  modifying existing vectors is a separate follow-up issue.)

## Acceptance criteria

- [ ] Vectors committed and passing in CI.
- [ ] Independent verification of 2–3 vectors documented in the PR.

## References

- inka-implementation-roadmap.md §0, §2 (M0 completion criterion, adapted), §5
