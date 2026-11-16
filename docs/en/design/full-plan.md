# inka — Full Implementation Plan (beyond the PoC)

Test evidence follows the [test evidence contracts](test-evidence.md). Producing and delivering
evidence JSON is the responsibility of the source repository and capsula; inka evaluates imported
evidence.

Capability areas that build on the PoC. The ordering below is
dependency-informed, **not a schedule**; nothing here commits to dates or
sequence beyond the stated dependencies.
Japanese version: [日本語版](https://www.space.t.u-tokyo.ac.jp/inka/ja/design/full-plan/).

## Capability areas

1. **Policy language.** Selectors (mode × id/namespace/`#tag`) and predicates
   (`any`, `role`, `including`, `not-author`, `fresh`, `forbidden`) parsed from
   a policy file referenced by `inka.toml`; `policy_hash` recorded in
   approvals; a selector matching no existing tag warns (the tag-typo
   detector). Replaces the built-in `any(1)`.
2. **Approval-session upgrades.** Diff against the local view the approver last
   approved (requires recovering the prior source); approver-side `until:`
   enforcement; a TUI (ratatui) if the CLI proves insufficient. The
   no-subtree-expansion rule holds in any UI.
3. **What-if evaluation.** `--forecast <phase>`, `--margin`,
   `--without-approver`, `--as-of`, `--diff-approvals` — all thin wrappers that
   re-run the pure eval with substituted inputs.
4. **LSP.** Workspace split (core / cli / lsp; core stays sync, no tokio);
   `lsp-server` + `lsp-types`; diagnostics, hover, inlay hints (children's
   descriptions at reference sites), read-only code lenses (approval state),
   goto/references, rename (the zero-invalidation demo). Semantic tokens
   supply rich description highlighting: the server parses each description
   range with a real CommonMark parser and emits tokens, superseding the
   deliberately simple TextMate grammar inside the editor.
5. **Signatures and integrity.** SSH signatures (ed25519, sshsig) over the
   canonical record bytes; `eval --verify`; lightweight anchoring (record
   count + hash chain) to make deletion tamper-evident; phase transitions as
   approvable events (`eval-clean` predicate + promotion attestations).
6. **Evidence-provider extensions.** Function-granularity test evidence
   (source-side adapter collection; reviewable source with CRLF-only normalization);
   execution-closure hashing recorded per evidence item, surfacing
   effective-content changes behind a stable interface as an eval warning,
   with an opt-in `closure-fresh` policy predicate for selective strictness.
   Signed CI attestations (OIDC/Sigstore) attach through the signature
   machinery.
7. **Calc evidence (veriq integration).** A `calc:` evidence kind for
   calculation graphs, veriq first ([calc evidence](calc-evidence.md)): a
   Python-side adapter exports each verification's definition-level upstream
   subgraph as reviewable raw source, the argument nodes it assumes (declared
   in node metadata, unioned over the subgraph) and its leaf results; inka
   hashes the sources CRLF→LF only; the argument itself is the assumption
   registry; the bind hash's assumption slot holds the assumed nodes'
   descriptions; assumptions are implicit dependencies of the bind;
   assumption-set diffs are presented at bind approval. Rust never parses
   Python — the JSON adapter is the entire seam.
8. **LLM lint.** A version-controlled prompt template; local-view-only review
   ("does the parent follow from the children?", "what presupposes knowledge
   outside the view?"); `llm:<model>` attestations recording template hash and
   model id; decline resolvable by a single human override with a logged
   reason.
9. **Cross-project reuse.** Export/import of bundles (nodes + attestations +
   originating policy); trust clauses on the importing side; portability rules
   (structure approvals travel; fiat must be re-accepted; bind must be re-bound
   and re-approved). Depends on signatures.
10. **Visualization.** A static single-file HTML export (topology, approval
    state, staleness; descriptions on click only), layout delegated to an
    existing JS graph library. Read-only; unreachable from approval flows.

## Standing technology choices

Hand-written recursive descent (no parser generator); `ratatui` + `crossterm`;
`lsp-server` + `lsp-types` (sync); `ed25519-dalek` + `ssh-key`; `ureq` for any
batch HTTP; `similar` for diffs; `insta` + `proptest` + `cargo-fuzz` for
testing. Deliberately absent: incremental-computation frameworks (full
recomputation until measured otherwise), async in core, and any binary format
— everything stays text.

## Discipline

- The dogfood argument is updated as each capability lands, and the approval
  loop is re-run on it — the tool's own argument stays its first user.
- Golden-vector directories stay append-only; incompatible layout changes ship
  as a parallel v2, never in place.
- Fuzz the parser and normalizer early (malformed UTF-8, huge inputs, deep
  reference chains).
