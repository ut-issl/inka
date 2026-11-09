/** What the hover tooltip says about the element under the pointer. */

import { nodeOf, type ArgumentView } from '../model/view'

/** What the pointer is over: a node, or an edge, each by its element id. */
export interface Hovered {
  readonly kind: 'node' | 'edge'
  readonly id: string
}

/**
 * The title and text for what is hovered: a node's id and contract, or an
 * entry's parent and child and its comment. An edge without a comment has
 * nothing to say, so it shows no tooltip.
 */
export function tooltipContent(
  view: ArgumentView,
  hovered: Hovered,
): { title: string; text: string } | null {
  if (hovered.kind === 'node') {
    if (!view.nodes.has(hovered.id)) return null
    const node = nodeOf(view, hovered.id)
    return { title: node.id, text: node.description }
  }
  const edge = view.edges.find((candidate) => candidate.id === hovered.id)
  if (edge?.comment === undefined) return null
  return { title: `${edge.source} → ${edge.target}`, text: edge.comment }
}
