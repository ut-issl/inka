# hash: implement local-view hashing (v1) as an executable specification

- **Labels:** enhancement
- **Milestone:** M0 (spec freeze — reinterpreted as code-first)
- **Depends on:** 04 (dogfood argument) — the contract data model must be validated
  by writing a real argument before v1 is frozen

## Motivation

The roadmap (M0) calls for writing `spec/hash.md` before any code. Instead, we make the
reference implementation itself the specification. The frozen artifact is the set of
golden test vectors plus the `v1` domain-separation tag — **not** a prose document.
The code can be refactored freely as long as the vectors keep passing.

Prose is ambiguous where code is not; property tests can pin down invariants
(order-independence, idempotence) that a document can only describe.

## Contract data model (prerequisite, settled by issue 04)

The hash is deliberately insulated from DSL *syntax*: IDs and tags are excluded and
child order is ignored precisely so that renames, reordering, and any future surface-
syntax change never invalidate an approval. What the hash **does** depend on is the
abstract contract model — what information a contract consists of (concept §3.1):

| kind | hash input |
|---|---|
| `structure(N)` | desc(N) + deadline annotations + multiset of children's descs |
| `bind(N)` | desc(N) + source statement + aggregated assumption set |
| `fiat(N)` | desc(N) + deadline annotations |

Issue 04 must have answered, before this issue starts:

1. component sufficiency (nothing else belongs to the approval interface),
2. the canonical form of deadline annotations as hash-input bytes,
3. multiset vs set semantics for children.

**Known limitation, accepted:** for `bind`, the *framing* (three sorted, length-
prefixed components) is frozen now, but the content of "source statement" and
"aggregated assumption set" is only pinned down by the veriq integration (M8).
Until then, bind vectors use synthetic strings.

## Design decisions to settle in this issue

Each decision must be recorded as **rustdoc on the module, with a one-line rationale**
(no separate design document):

1. **Input unit**: local view = normalized description of the node + deadline
   annotations + the *multiset* of the children's normalized descriptions.
2. **String normalization — one regime (every description is a fenced block;
   the DSL has no bare single-line form)**: UTF-8, NFC, CRLF → LF, strip per-line
   trailing whitespace, strip common indentation, strip leading/trailing blank
   lines; **internal newlines and spacing are preserved verbatim** ("what you see
   is what you approved"). No markdown-AST normalization — the frozen spec must
   not depend on CommonMark; the accepted cost is that reflowing a description
   changes its hash. (Comments are dropped at the lexer stage — out of scope here.)
3. **Child ordering**: sort children by their normalized-description bytes, so the hash
   is order-independent (reordering `supported-by` invalidates nothing).
4. **Framing**: length-prefixed concatenation (`len || bytes`) to remove delimiter
   ambiguity, prefixed with a domain-separation tag (`"inka:structure:v1"`,
   `"inka:bind:v1"`, `"inka:fiat:v1"`), then SHA-256.
5. **Exclusions / inclusions**: IDs and tags (`#safety`) are excluded from the hash;
   deadline annotations (`until:` / `promote:`) are included, in the canonical form
   decided in issue 04.
6. **Versioning**: `v1` is baked into the domain-separation tag. Any future change to
   the normalization rules must ship as `v2` side by side, never as an in-place edit.
   (Escape hatch: until real approval records start accumulating — M3 dogfooding —
   a v1 revision only costs regenerating vectors, not anyone's approvals.)

## Implementation notes

- A pure function over a `LocalView` struct (plain strings). **No parser dependency** —
  the input is already-extracted descriptions/annotations, so this module lands before
  the DSL parser exists (this moves hashing ahead of the roadmap's M2 ordering).
- Descriptions in `LocalView` are plain strings; the DSL has a single description
  form (fenced block), so no form tag is needed.
- Value interpolation (`{$var}` resolved from the values TOML) happens during
  extraction, **before** normalization/hashing — `LocalView` receives fully rendered
  descriptions and the hash never sees placeholders. Consequently, changing a value
  invalidates exactly the approvals of contracts referencing it (plus their direct
  parents' structure approvals).
- Crates: `sha2`, `unicode-normalization`.
- Compute all three view-hash kinds: `structure` / `bind` / `fiat`.

## Property tests (proptest)

- Hash is invariant under indentation shift, per-line trailing whitespace, and
  CRLF/LF differences — but **changes** when internal newlines or spacing change.
- Hash is invariant under permutation of children.
- Any single-character change to a description changes the hash.
- Normalization is idempotent: `normalize(normalize(x)) == normalize(x)`.

## Acceptance criteria

- [ ] Module with the pure hashing function merged.
- [ ] All six design decisions documented in rustdoc with a one-line rationale each.
- [ ] Property tests above pass.

## References

- inka-concept.md §3.1 (binding targets), §3.3 (hash normalization rules, frozen in v0)
- inka-implementation-roadmap.md §0 ("freeze specs as test vectors, not code"), §2, M2