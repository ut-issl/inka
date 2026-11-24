# inka — Web UI

Invariants the Web UI holds. It is one application with two surfaces, built on the same shared model as every
other output: a **review surface**, where an approver walks the review queue and records decisions as
`inka review` does, and a **graph surface**, where the whole argument is explored.
Japanese version: [日本語版](https://www.space.t.u-tokyo.ac.jp/inka/ja/design/web-ui/).

## Foundation

- The Web UI shows the shared argument view — the same projection every other output is built on — and the
  review queue the core derives from it. It never parses the argument, and it computes no hash, no review state
  and no queue order of its own: those come from the core, through the server, exactly as `inka review` and
  `inka eval` read them.
- The server keeps no state of its own. Every answer is derived from the repository's files at the time of the
  request, so the page and the command line never disagree about what the project says. A project that fails
  `check` is shown as its diagnostics, never as a partial argument.
- The only thing the Web UI can change is the approval log, and only by appending one record
  (see [Recording a decision](#recording-a-decision)). Nothing in it edits the argument, the values, the
  evidence, or an existing record.
- The argument is a DAG. Every node appears exactly once in the graph; a node with multiple parents is shown
  through edges, never by duplication.
- Rendering is deterministic: identity and ordering derive from the view's sorted block-ID order and from the
  queue's order, so the same repository content always renders the same pages.
- Implementation: the renderer is a single TypeScript front-end bundle (`web/`, the one source of these
  invariants), served by `inka serve` (Rust). The graph library is vendored into the bundle, not part of inka.

## The local view

The local view is the unit both surfaces present a node in. It is what an approval binds to
(`concept.md` core idea 2), so it is what a reviewer reads.

- A node's local view shows, in this order: the comment on the block, the description, the evidence reference,
  the direct children's descriptions — each with the comment on its entry — and the deadline annotations, each
  property with the comment attached to it. This is the content and the order `inka review` prints.
- The description under judgement is visually set apart from the children's descriptions around it.
- Nothing deeper is shown, and nothing is expanded in place: a child appears as its description only. Reading
  below a child means navigating to that child's own local view.
- Everything that enters the view hash is on screen without a hover, a click or a scroll inside a nested box:
  no contract content is reachable only through an interaction.
- The header names the target (`<mode>:<id>`) and its review state. The state is always spelled out; colour
  repeats it and never carries it alone.

## Contract text

- A description is rendered as CommonMark. Comments, ids and evidence targets are plain text.
- Rendering never hides or alters contract content. Raw HTML in a description is shown as text, not
  interpreted. No remote resource is loaded: an image is shown as its alternative text and destination, and a
  link shows where it leads.
- The description's source — the interpolated, normalized text the hash is computed from — is one control away
  from its rendering, so whitespace and line breaks that Markdown folds remain inspectable.
- Text from the argument is never executed or trusted as markup beyond CommonMark; control and bidirectional
  override characters are neutralized as `inka review` neutralizes them.

## Review surface

- The queue is the one `inka review --list` prints: the same targets, grouped by state — unapproved, stale,
  declined, blocked — and ordered from the leaves up within each group.
- A session walks the targets that can take a decision — unapproved and stale — one local view at a time, in
  queue order. Declined and blocked targets are listed with what resolves them and offer no decision.
- A stale target names who approved the earlier content; a declined one names who declined.
- The reviewer may approve, decline, or skip. Any queued target can also be opened directly; the queue's order
  is a recommendation, not a gate.
- Progress — the position in the queue and what remains — is always visible.

## Recording a decision

A decision recorded in the browser is the record `inka review` writes: the same fields, the same file, appended
by the same code path. Nothing in the record tells the two apart.

- **Opt-in.** Recording exists only when the server was started with an approver (`inka serve --as <approver>`).
  Without one the server offers no route that writes, and the Web UI is read-only throughout.
- **Identity is the server's.** The approver, and the key id, are fixed when the server starts. The page shows
  who it records as at all times and cannot choose or change it.
- **Loopback only.** A server that records listens on a loopback address; asking for recording on any other
  address is refused at startup.
- **Own pages only.** A decision is accepted only from a page this server served for this run — another site
  open in the same browser cannot cause a record to be written.
- **The hash on screen.** A decision names the view hash of the local view that was displayed. The server
  records it only if that is still the target's current hash; otherwise nothing is written and the page shows
  the current content. An edit made while the page is open can never be approved unseen.
- **The whole local view.** Approve and decline are offered only on a local view shown in full, for a target
  that can take a decision. No control anywhere else — a graph card, a list entry, a hover — records anything.
- **Decline needs a reason.** A decline requires a comment; an approval may carry one.
- **Append-only.** One decision appends one record. No retraction, edit or deletion of a record is offered.

## Graph surface

- Every node is a card that carries its mode, its id, its review state and its description, rendered as in the
  local view. A long description may be clipped on the card; the node's local view shows it whole.
- The four discharge modes (objective / structure / bind / fiat) are visually distinct and also named on the
  card.
- The layout is a top-down layered DAG — objectives at the top, edges flowing downward. An edge points from a
  parent to each of its children (arrowhead at the child), and every edge is drawn with the same weight.
- An edge whose `children:` entry carries a comment shows that it does; the comment itself is read in the
  parent's local view.

### Exploring

- The graph opens collapsed: the objectives and their direct children are visible.
- A node with children is expanded or collapsed on its own. Expanding shows its direct children; collapsing
  hides every descendant that is not also reachable through another expanded, visible node.
- A collapsed node says that it has hidden children and how many nodes lie below it, so nothing is hidden
  silently.
- A node is visible exactly when it is an objective or a child of an expanded, visible node. Whether a node is
  visible never depends on the order in which things were expanded.
- The whole graph can be expanded, and collapsed again, in one action.
- Revealing a node from elsewhere — a search result, a queue entry, a link in a local view — expands a path
  from an objective down to it.

### Selecting

- Selecting a card opens that node's local view beside the graph; the graph stays in place. There is no
  separate one-hop graph for a node: the local view is the node's close-up.
- The local view lists the node's parents as supplementary navigation, set apart from the content the approval
  binds to.
- Esc, or a tap on the empty background, clears the selection.

## Supplementary information

Context that helps a reviewer but is not part of what an approval binds to. It is always visually separate from
the local view's contract content.

- **Sidebar.** On both surfaces a sidebar lists the review queue and every node of the argument, and finds
  nodes by id and by description text. An entry shows the node's mode and review state; choosing one opens the
  node.
- **Hover.** Hovering a reference to a node — a child in a local view, a parent, a sidebar entry, a clipped
  card — previews that node's mode, id, state and description. A preview never offers a decision, and nothing
  is reachable only through one.
- **Namespace.** A node's namespace is shown with its id wherever the node is named. Until namespace resolution
  lands, a node's namespace is its argument-file stem.
- **Links between surfaces.** A local view on the review surface can be located in the graph, and a queued
  node selected in the graph can be opened in the review surface.

## Not invariant

- Colours, card dimensions, how many lines a card shows before clipping, spacing, zoom behaviour, sidebar
  layout and keyboard shortcuts are presentation parameters, tuned freely; they are not invariants.

## References

- `concept.md` core idea 1 (the four discharge modes the cards make visible) and core idea 2 (the local view an
  approval binds to)
- `spec.md` Comments (presentation at review), Approval records (the record a decision appends)
- `full-plan.md` capability 2 (approval-session upgrades) and capability 10 (visualization)
