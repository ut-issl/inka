/**
 * The shared argument view: the one structure both the graph and the node list
 * read from, so they cannot disagree about content or ordering.
 *
 * Nothing here decides anything. The nodes, their parents and children, the
 * parents-first order and the namespace groups all arrive from `inka serve`
 * already worked out (see `crate/inka-core/src/wire.rs`); this module only
 * indexes them by id and lays the parent-to-child entries out as a list of
 * edges for the graph to draw.
 */

import type { NamespaceGroup } from '../generated/NamespaceGroup'
import type { NodeData } from '../generated/NodeData'
import type { Snapshot } from '../generated/Snapshot'

/** One block, as the server describes it. */
export type ArgumentNode = NodeData

export type { NamespaceGroup }

/** One `children:` entry: a parent, a child, and the comment on the entry. */
export interface Edge {
  readonly id: string
  readonly source: string
  readonly target: string
  readonly comment: string | null
}

export interface ArgumentView {
  /** Every node, by id, in sorted block-ID order. */
  readonly nodes: ReadonlyMap<string, ArgumentNode>
  /** Every node id, parents before children. */
  readonly order: readonly string[]
  /** The nodes grouped by namespace. */
  readonly groups: readonly NamespaceGroup[]
  /** Every entry, parent by parent in node order, in declaration order. */
  readonly edges: readonly Edge[]
}

/** Raised when a node is asked for that the view does not have. */
export class ViewError extends Error {
  override readonly name = 'ViewError'
}

/** Indexes a snapshot for the surfaces that read it. */
export function viewOf(snapshot: Snapshot): ArgumentView {
  const nodes = new Map(snapshot.nodes.map((node) => [node.id, node]))
  const edges = snapshot.nodes.flatMap((node) =>
    (node.local.children?.entries ?? []).map((entry) => ({
      id: `${node.id}->${entry.id}`,
      source: node.id,
      target: entry.id,
      comment: entry.comment,
    })),
  )
  return { nodes, order: snapshot.order, groups: snapshot.namespaces, edges }
}

/** The namespaces of the view, in group order. */
export function namespacesOf(view: ArgumentView): string[] {
  return view.groups.map((group) => group.name)
}

/** The node with this id. */
export function nodeOf(view: ArgumentView, id: string): ArgumentNode {
  const node = view.nodes.get(id)
  if (!node) throw new ViewError(`unknown node \`${id}\``)
  return node
}

/** The objectives the argument hangs from, in view order. */
export function rootsOf(view: ArgumentView): ArgumentNode[] {
  return view.order.map((id) => nodeOf(view, id)).filter((node) => node.root)
}
