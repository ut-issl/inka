# inka web UI

The Web UI of an inka argument: a review surface that walks the review queue one local view at a time, and a
graph of the whole argument. Read-only.
This bundle is the single implementation of the Web UI invariants in
[`docs/design/web-ui.md`](../docs/en/design/web-ui.md) — behaviour described
there is decided here, and nowhere else.

Vite + React + TypeScript. Descriptions are parsed and rendered by `react-markdown`, routing between the
surfaces is `react-router`'s, and the graph still uses cytoscape and dagre; all are vendored into the bundle.
`inka serve` ([#43](https://github.com/ut-issl/inka/issues/43)) serves the built
files and the page that describes the argument. Nothing here parses or mutates the argument, and no
control in the UI reaches a write of any kind.

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
`npm run e2e` starts its own `inka serve` on the aocs fixture and its own dev server, each on a port of its
own, and never reuses one that is already running.

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
  model/      the argument, independent of any rendering
    view      the shared view: the page's nodes by id, and its entries as edges
    modes     the order the discharge modes are listed in
    focus     focus targets and the sets each surface derives from them
    source    fetching the page
    wording   how a status and a review state are spelled out
  text/       neutralizing text from the argument before it is shown
  review/     the review surface: the queue and the walk over it
  graph/      the graph pane
    layout    pure layout arithmetic — ranks, ego rows, aspect matching
    runLayout the cytoscape adapter feeding those passes
    stylesheet, palette, elements, GraphCanvas, NamespaceBadges, HoverTooltip
  components/ the local view, descriptions (Markdown and source), status pills;
              and the graph surface's node list, detail panel, legend
  hooks/      loading state
e2e/          the same invariants, in a real browser
```

What the argument *is* and in what order it is presented is decided in Rust and arrives in the page.
The invariants worth stating in code here live in two places: `model/focus.ts` decides what a focus
shows, and `graph/layout.ts` decides where things go. Neither knows about React or cytoscape, so both
are tested directly.

## How the invariants are covered

Two suites, split by what each can see. Vitest covers everything that does not
need a canvas — `model/` and `graph/layout.ts` sit at 100 % and 99 % statement
coverage. The graph pane itself (cytoscape, the badge overlay, the hover
tooltip, the layout adapter) can only be exercised in a browser, so Playwright
covers it instead; those four files are why the overall Vitest number is ~66 %.

| Invariant | Where it lives | Covered by |
| --- | --- | --- |
| Local view: the content and order `inka review` prints, nothing deeper | `components/LocalView.tsx` | `LocalView.test.tsx`, `review.spec.ts` |
| Markdown never hides content: raw HTML as text, no remote resource, link destinations shown | `components/Markdown.tsx` | `Markdown.test.tsx` |
| A description's source is one control away | `components/Description.tsx` | `LocalView.test.tsx`, `review.spec.ts` |
| Control and bidirectional characters neutralized | `text/neutralize.ts` | `neutralize.test.ts` |
| Queue as `inka review --list`; walk in queue order; any target opens directly | `review/ReviewSurface.tsx` | `ReviewSurface.test.tsx` |
| Status and review state spelled out, never colour alone | `model/wording.ts` | `LocalView.test.tsx` |
| Deterministic identity and ordering | `inka-core` `wire.rs` | its Rust tests; `fixtures.test.ts` |
| One node per block; multi-parent nodes shown through edges | `model/view.ts` | `view.test.ts`, `fixtures.test.ts` |
| Focus targets: one node, or a namespace with its one-hop neighbours | `model/focus.ts` | `focus.test.ts` |
| Same-namespace nodes adjacent, one namespace order across ranks | `graph/layout.ts` | `layout.test.ts` |
| Aspect ratio matches the window, short axis only | `graph/layout.ts` | `layout.test.ts`, `viewer.spec.ts` |
| Mode encoded by shape and fill; label is the node id | `graph/stylesheet.ts` | `stylesheet.test.ts` |
| Top-down layered DAG; ego rows on node focus | `graph/runLayout.ts` | `viewer.spec.ts` |
| Badges on every node, hidden below a zoom threshold | `graph/NamespaceBadges.tsx` | `viewer.spec.ts` |
| Hover reveals the full description | `graph/HoverTooltip.tsx` | `viewer.spec.ts` |
| List grouped by namespace, both orders topological | `components/NodeList.tsx` | `NodeList.test.tsx` |
| Contract, evidence target and deadline on selection | `components/DetailPanel.tsx` | `DetailPanel.test.tsx` |
| Esc and background tap restore the whole view | `App.tsx`, `GraphCanvas.tsx` | `viewer.spec.ts` |
| Read-only throughout | everywhere | `NodeList.test.tsx`, `viewer.spec.ts`, `review.spec.ts` |
