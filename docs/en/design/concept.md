# inka — Concept

What inka is and the ideas everything else follows from.
Japanese version: [日本語版](https://www.space.t.u-tokyo.ac.jp/inka/ja/design/concept/).

## Problem

Assurance arguments for iteratively developed systems rot. Approvals are
ceremonies attached to documents; when something changes, either everything is
treated as invalidated or nothing is; and the records are not machine-checkable.
inka makes the argument, its approvals, and their invalidation *code*: text in a
repository, evaluated by a pure function.

## Core ideas

1. **There are only propositions.** A node is a proposition with a
   natural-language contract (its description). It is *discharged* in one of
   three modes, declared as the block keyword: `structure` (decomposed into
   children), `bind` (bound to machine-verifiable evidence), or `fiat` (accepted
   without argument; leaves only). The root is a single `objective`; its
   wording is itself an approvable contract — agreement on the root is recorded
   inside inka like everything else. Modes map one-to-one onto approval targets.

2. **Approvals bind to the local view.** The local view of a node is its own
   rendered description + deadline annotations + its direct children's
   descriptions — nothing deeper. Every approval record stores a hash of that
   view. Consequences: editing a description invalidates exactly the node's own
   approvals and its direct parents' structure approvals, nothing else; renames
   and child reordering invalidate nothing (IDs, tags, and comments are not part
   of the contract); re-approval cost is proportional to the diff.

3. **Truth and validity are separated.** Whether evidence *passes* is machine
   truth, re-established by re-executing the evidence; eval only reconciles
   recorded results. Whether that evidence is *appropriate* for the proposition
   is human validity — the bind approval. Assumptions the evidence relies on
   aggregate through interfaces and surface at the evidence boundary.

4. **Approval semantics.** Labels are approve / decline /
   cannot-take-responsibility. A decline on the current hash is a veto: quorum
   never overrides it. `established` is a fixpoint over the DAG (fiat approval ∨
   structure approval ∧ all children established ∨ bind approval ∧ evidence
   pass; the objective needs its own approval plus all children). eval reads
   repository content only — no clock, no network — so the
   same commit always evaluates identically.

5. **Policy is reviewable text.** Which targets need whose approvals is written
   as selectors (mode × id/namespace/`#tag`) and predicates (`any`, `role`,
   `including`, `not-author`, `fresh`, `forbidden`, …). Phases have a declared
   total order; `fresh` compares recorded phases, never timestamps. Deadline
   annotations let authors declare debt that expires: `until: <phase>` with a
   quoted `reason:` on any node (`reason:` and `promote:` only ever accompany
   an `until:`), plus `promote:` to name the successor mode — `structure` or
   `bind` on a fiat, `structure` only on the objective. Expiry is hard — past the deadline no approval of
   the node counts, even freshly granted ones — so keeping things as they are
   requires visibly editing the annotation (a contract change), never silent
   re-approval.

6. **Values are contracts too.** `{$var}` in descriptions resolves from a TOML
   file; contracts are hashed *after* interpolation, so changing one requirement
   value precisely invalidates every contract that references it.

7. **Records are auditable.** Approvals are append-only jsonl, one
   self-contained record per line, carrying the phase at approval time.
   Signatures and anchoring strengthen integrity later; the schema reserves
   their fields from day one.

## Lifecycle

A proposition typically starts `fiat` with `until:` / `promote:` / `reason:`
(explicit, expiring debt), then becomes `bind` when evidence exists, or
`structure` when decomposed. A structure or bind whose grounds feel weak
carries `until:` + `reason:` as an obligation to revise its children or source
by the deadline. Because parents bind only to children's descriptions, neither
a child's mode change nor its annotations ever invalidate the parent's
approval.
