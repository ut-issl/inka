/**
 * Turns the shared view into cytoscape elements.
 *
 * The graph is built from the view rather than from the raw export, so the
 * graph and the node list cannot drift: both read the same nodes, in the same
 * order, with the same neighbours.
 */

import type { ElementDefinition } from 'cytoscape'

import { nodeOf, type ArgumentView } from '../model/view'

export function cytoscapeElements(view: ArgumentView): ElementDefinition[] {
  const nodes: ElementDefinition[] = view.order.map((id) => {
    const node = nodeOf(view, id)
    return {
      group: 'nodes',
      // Only what the stylesheet selects on: everything else is read from the
      // view, so node data cannot become a second source of truth.
      data: { id: node.id, mode: node.mode, namespace: node.namespace },
    }
  })
  const edges: ElementDefinition[] = view.edges.map((edge) => ({
    group: 'edges',
    data: { id: edge.id, source: edge.source, target: edge.target },
  }))
  return [...nodes, ...edges]
}
