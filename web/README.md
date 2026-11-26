# inka web UI

The Web UI of an inka argument: a review surface that walks the review queue one local view at a time, and a
graph of the whole argument. Read-only unless `inka serve` was started with an approver; then the review
surface records decisions, and that is the only thing the Web UI can change.
This bundle is the single implementation of the Web UI invariants in
[`docs/design/web-ui.md`](../docs/en/design/web-ui.md) — behaviour described
there is decided here, and nowhere else.

Vite + React + TypeScript. What a library already does is left to it: descriptions are parsed and rendered by
`react-markdown`, routing between the surfaces is `react-router`'s, the graph's canvas — pan, zoom, edges,
cards as DOM elements — is React Flow's (`@xyflow/react`), and the layout is computed by `elkjs`. All are
vendored into the bundle, with the Inter typeface, so text measures the same on every machine.
`inka serve` ([#43](https://github.com/ut-issl/inka/issues/43)) serves the built
files and the page that describes the argument. Nothing here parses or mutates the argument; the one
write the UI can reach is a decision, on a server that records.

## Getting started

The bundle is always developed against a real `inka serve`: the dev server forwards `/api` to it and answers
nothing about the argument itself. Start one from the repository root, then the dev server:

```sh
# In one terminal, from the repository root:
cargo run -p inka-cli -- serve tests/fixtures/aocs   # http://127.0.0.1:8080

# In another, from web/ — as every npm command below:
npm install
npm run dev                      # http://localhost:5173
INKA_SERVE=http://127.0.0.1:9000 npm run dev   # when `inka serve` listens elsewhere
npm test                         # unit and component tests
npm run coverage
npm run e2e                      # browser tests (needs `npx playwright install chromium`)
npm run build                    # static bundle in ../crate/inka-cli/web-dist/, embedded by `inka`
```

Edit the project `inka serve` points at and reload the page: the server reads it again on every request.
`npm run e2e` starts its own servers, each on a port of its own, and never reuses one that is already running:
a read-only `inka serve` on the aocs fixture, a recording one on a throwaway copy of it, and a dev server in
front of each.

## The interface with inka

The viewer consumes one payload: the **page** `inka serve` answers `GET ./api/page` with. Its definition is in
Rust — `crate/inka-core/src/wire.rs` — and nowhere else. A page is either the argument or, for a project that
fails `inka check`, the findings that rejected it:

```jsonc
{ "kind": "argument",
  "established": 1, "total": 17,
  "nodes": [
    { "id": "detumble_ok", "mode": "structure", "namespace": "safety", "tags": ["safety"], "root": false,
      "parents": ["aocs_ok"], "children": ["detumble_convergence_ok", "mtq_authority_ok"],
      "below": 3, "blamed_below": 3,
      "status": { "state": "blamed", "reasons": [{ "cause": "missing_approval" }] },
      "review": { "state": "unapproved" },
      "view_hash": "sha256:…",
      "local": { "description": "…", "children": { "comment": null, "entries": [ … ] }, … } }
  ],
  "roots": ["aocs_ok"],
  "order": ["aocs_ok", "detumble_ok", … ],
  "namespaces": [{ "name": "objective", "nodes": ["aocs_ok"] }, … ],
  "queue": { "walk": [ … ], "groups": [{ "state": "unapproved", "targets": [ … ] }] },
  "warnings": [] }
```

```jsonc
{ "kind": "rejected",
  "findings": [{ "severity": "error", "message": "…",
                 "location": { "path": "argument/a.inka", "line": 3, "column": 5 } }] }
```

Everything that carries meaning arrives worked out: each node's parents, the parents-first `order`, the
namespace groups, the verdict of `inka eval`, the review queue, and each node's local view. The bundle indexes
and draws; it derives none of it. Descriptions arrive interpolated; no `{$var}` reaches the viewer.

The page is fetched on load and never cached or embedded in the served HTML: it is a reading of the repository
at the time of the request.

### Recording a decision

`GET ./api/session` says who the server records as: `{ "reviewer": null }` on a read-only server, or the
approver, the key id and a session token on one started with `--as`. On such a server a decision is one request:

```jsonc
// POST ./api/decisions, with the token in `X-Inka-Session`
{ "target": "detumble_ok", "view_hash": "sha256:…", "decision": { "label": "approve", "comment": null } }
// → { "outcome": "recorded" } | { "outcome": "changed" } | { "outcome": "refused", "message": "…" }
```

`view_hash` is the hash of the local view that was on screen; `changed` means the target no longer has that
content, so nothing was written and the page asks for the current one. The request cannot say who is deciding.
The server refuses a request that is not addressed to it or does not come from one of its own pages, so in
development the proxy in `vite.config.ts` addresses requests to the real server and names it as their origin.

### Generated from Rust

Two things here are generated from the Rust definition and never edited:

- `src/generated/*.ts` — the TypeScript types of the page, one file per Rust type. The bundle is embedded in
  the `inka` that answers it, so the two always match, and nothing in `src/` re-states or re-validates the
  shape.
- `fixtures/*.page.json` — the page for each of the two example projects, which the unit tests read.
  `src/model/fixtures.test.ts` holds them to what the viewer relies on.

`cargo test -p inka-core` fails when either is out of date. Regenerate both from the repository root:

```sh
INKA_UPDATE_GENERATED=1 cargo test -p inka-core
```

## Layout

```text
src/
  generated/  the page's types, generated from Rust
  model/      the page, independent of any rendering
    view      the page's nodes by id
    modes     the order the discharge modes are listed in
    source    fetching the page
    session   who the server records as, and posting a decision
    wording   how a status and a review state are spelled out
  text/       neutralizing text from the argument before it is shown
  review/     the review surface: the queue, the walk over it, the decision controls
  graph/      the graph surface
    visibility which nodes are shown, given what is expanded
    layout     where the cards go: the question put to ELK, and its answer
    Card, CardGraph, GraphSurface
  components/ the local view, descriptions (Markdown and source), status pills
  hooks/      the page and the session
e2e/          the same invariants, in a real browser
```

What the argument *is* and in what order it is presented is decided in Rust and arrives in the page.
Two things are decided here, both view state, both pure functions tested without a browser:
`graph/visibility.ts` says which nodes are on screen for a given set of expanded nodes, and
`graph/layout.ts` says where the cards go for given sizes.

## How the invariants are covered

Two suites, split by what each can see. Vitest covers everything that does not need a real layout engine: the
models, the review surface, and the two graph functions above. The graph's canvas needs a browser to measure
its cards, so Playwright covers it — and because the cards are DOM elements it asserts geometry (no two cards
overlap, a child is below its parent), never pixels.

| Invariant | Where it lives | Covered by |
| --- | --- | --- |
| Local view: what `inka review` prints, nothing deeper | `components/LocalView.tsx` | `LocalView.test.tsx` |
| Markdown hides nothing: raw HTML as text, no remote resource | `components/Markdown.tsx` | `Markdown.test.tsx` |
| A description's source is one control away | `components/Description.tsx` | `LocalView.test.tsx` |
| Control and bidirectional characters neutralized | `text/neutralize.ts` | `neutralize.test.ts` |
| Queue as `inka review --list`; walk in queue order | `review/ReviewSurface.tsx` | `ReviewSurface.test.tsx` |
| Status and review state spelled out, never colour alone | `model/wording.ts` | `LocalView.test.tsx` |
| Decisions only on a recording server, under a whole local view | `review/ReviewSurface.tsx` | `Decisions.test.tsx` |
| A decision names the hash on screen; identity is the server's | `model/session.ts` | `session.test.ts`, `serve.rs` |
| A decline needs a reason; one decision is one append | `review/DecisionBar.tsx` | `recording.spec.ts` |
| Deterministic identity and ordering | `inka-core` `wire.rs` | its Rust tests; `fixtures.test.ts` |
| One card per node; a multi-parent node appears once | `graph/visibility.ts` | `visibility.test.ts`, `graph.spec.ts` |
| Visible iff a root or a child of an expanded, visible node | `graph/visibility.ts` | `visibility.test.ts` |
| Unreachable nodes drawn from roots of their own | `graph/visibility.ts`, `wire.rs` | `visibility.test.ts` |
| Layout is a function of sizes and edges; no overlap | `graph/layout.ts` | `layout.test.ts`, `graph.spec.ts` |
| Top-down: every child below its parent | `graph/layout.ts` | `layout.test.ts`, `graph.spec.ts` |
| A card carries mode, id, status and description | `graph/Card.tsx` | `graph.spec.ts` |
| A collapsed node says what lies below it | `graph/Card.tsx` | `graph.spec.ts` |
| A card records nothing | `graph/Card.tsx` | `graph.spec.ts` |
| Read-only without `--as` | everywhere | `Decisions.test.tsx`, `graph.spec.ts`, `review.spec.ts` |
