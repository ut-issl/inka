# inka web UI

The interactive graph viewer: a read-only visualization of an inka argument.
This bundle is the single implementation of the Web UI invariants in
[`docs/design/web-ui.md`](../docs/en/design/web-ui.md) — behaviour described
there is decided here, and nowhere else.

Vite + React + TypeScript, with cytoscape and dagre vendored into the bundle.
`inka serve` ([#43](https://github.com/ut-issl/inka/issues/43)) serves the built
files and the argument JSON. Nothing here parses or mutates the argument, and no
control in the UI reaches a write of any kind.

## Getting started

```sh
npm install
npm run dev                      # http://localhost:5173, on the aocs fixture
INKA_FIXTURE=dogfood npm run dev # the dogfood fixture instead
npm test                         # unit and component tests
npm run coverage
npm run e2e                      # browser tests (needs `npx playwright install chromium`)
npm run build                    # static bundle in dist/
```

`npm run dev` needs no Rust side: the dev server answers `/elements.json` from
`fixtures/<project>.elements.json`.

## The interface with inka

The viewer consumes one payload: the cytoscape `elements` array that
`inka export` ([#49](https://github.com/ut-issl/inka/issues/49)) writes, built
from the shared view model ([#48](https://github.com/ut-issl/inka/issues/48)).
[`src/model/elements.ts`](src/model/elements.ts) is its executable
specification — the schema there is what the viewer accepts, and every payload
is validated against it before anything is drawn.

```jsonc
[
  { "data": { "id": "detumble_ok", "mode": "structure", "namespace": "safety",
              "description": "…", "tags": ["safety"], "root": false } },
  { "data": { "id": "detumble_convergence_ok", "mode": "bind", "namespace": "safety",
              "description": "…", "tags": [], "root": false,
              "evidence": { "kind": "veriq", "target": "aocs.verify_detumble" } } },
  { "data": { "id": "mtq_authority_ok", "mode": "fiat", "namespace": "safety",
              "description": "…", "tags": [], "root": false,
              "deadline": { "until": "FM", "promote": "bind", "reason": "…" } } },
  { "data": { "id": "detumble_ok->mtq_authority_ok",
              "source": "detumble_ok", "target": "mtq_authority_ok" } }
]
```

Two properties of the array are load-bearing, and hold by construction on the
Rust side:

- node elements come in **sorted block-ID order**. Every deterministic ordering
  in the UI — the topological order, the node list's groups, the namespace
  palette — derives from it, so the same project always renders the same graph;
- edge elements come in each parent's **declaration order**.

There is no `label` field: a node's label *is* its id, so the graph reads
`data(id)` and that invariant cannot drift. Descriptions arrive interpolated;
no `{$var}` reaches the viewer.

### Delivery

`inka serve` may deliver the payload either way; the viewer prefers the first:

1. **injected** into the served HTML, saving a round trip:
   `<script type="application/json" id="inka-elements">[ … ]</script>`;
2. **fetched** from `./elements.json`, relative to the page. Override the path
   with `VITE_INKA_ELEMENTS` at build time.

### Fixtures

`fixtures/*.elements.json` are the two example projects, checked in so the UI
runs and is tested without the Rust side. Until `inka export` lands they are
maintained by hand; once it does, regenerate them from the projects themselves:

```sh
cargo run -p inka-cli -- export tests/fixtures/aocs > web/fixtures/aocs.elements.json
```

`src/model/fixtures.test.ts` holds them to the contract, so drift between the
exporter and this bundle fails the test suite rather than the browser.

## Layout

```text
src/
  model/      the argument, independent of any rendering
    elements  the `inka export` contract and its validation
    view      the shared view: nodes, neighbours, topological order, groups
    focus     focus targets and the sets each surface derives from them
    source    where the payload comes from (injected or fetched)
  graph/      the graph pane
    layout    pure layout arithmetic — ranks, ego rows, aspect matching
    runLayout the cytoscape adapter feeding those passes
    stylesheet, palette, elements, GraphCanvas, NamespaceBadges, HoverTooltip
  components/ node list, detail panel, legend
  hooks/      loading state
e2e/          the same invariants, in a real browser
```

The invariants worth stating in code live in two places: `model/` decides what
the argument *is* and in what order it is presented, and `graph/layout.ts`
decides where things go. Neither knows about React or cytoscape, so both are
tested directly.

## How the invariants are covered

Two suites, split by what each can see. Vitest covers everything that does not
need a canvas — `model/` and `graph/layout.ts` sit at 100 % and 99 % statement
coverage. The graph pane itself (cytoscape, the badge overlay, the hover
tooltip, the layout adapter) can only be exercised in a browser, so Playwright
covers it instead; those four files are why the overall Vitest number is ~66 %.

| Invariant | Where it lives | Covered by |
| --- | --- | --- |
| Deterministic identity and ordering | `model/view.ts` | `view.test.ts`, `fixtures.test.ts` |
| One node per block; multi-parent nodes shown through edges | `model/view.ts` | `view.test.ts` |
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
| Read-only throughout | everywhere | `NodeList.test.tsx`, `viewer.spec.ts` |
