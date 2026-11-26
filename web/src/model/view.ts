/**
 * The page, indexed for the surfaces that draw it.
 *
 * Nothing here decides anything. The nodes, their parents and children and
 * the parents-first order all arrive from `inka serve` already worked out
 * (see `crate/inka-core/src/wire.rs`); this module only lets a node be looked
 * up by its id.
 */

import type { NodeData } from '../generated/NodeData'
import type { Snapshot } from '../generated/Snapshot'

/** One block, as the server describes it. */
export type ArgumentNode = NodeData

export interface ArgumentView {
  /** Every node, by id, in sorted block-ID order. */
  readonly nodes: ReadonlyMap<string, ArgumentNode>
  /** Every node id, parents before children. */
  readonly order: readonly string[]
}

/** Raised when a node is asked for that the view does not have. */
export class ViewError extends Error {
  override readonly name = 'ViewError'
}

/** Indexes a snapshot for the surfaces that read it. */
export function viewOf(snapshot: Snapshot): ArgumentView {
  return { nodes: new Map(snapshot.nodes.map((node) => [node.id, node])), order: snapshot.order }
}

/** The node with this id. */
export function nodeOf(view: ArgumentView, id: string): ArgumentNode {
  const node = view.nodes.get(id)
  if (!node) throw new ViewError(`unknown node \`${id}\``)
  return node
}
