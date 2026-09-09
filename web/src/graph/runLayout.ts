/**
 * Adapter between cytoscape and the pure layout passes.
 *
 * Two layouts exist, one per state: the layered DAG (whole view and namespace
 * focus) and the three ego rows (node focus). Both end the same way — reorder
 * ranks, stretch to the window's aspect ratio, fit — so every state fills the
 * graph window the same way.
 *
 * The pre-stretch positions are handed back as the layout's *base*. A resize
 * re-stretches from that base rather than from the stretched result, so
 * repeatedly resizing the window cannot inflate the graph step by step.
 */

import type { Core, NodeSingular } from 'cytoscape'

import { LAYOUT } from '../config'
import {
  egoRows,
  orderRanks,
  stretchToAspect,
  type LayoutNode,
  type Positions,
} from './layout'

export type LayoutPlan =
  | { readonly kind: 'layered' }
  | { readonly kind: 'ego'; readonly focusId: string; readonly namespaces: readonly string[] }

/** Reads the visible nodes as layout input, sizes included. */
function visibleNodes(cy: Core): LayoutNode[] {
  return cy
    .nodes(':visible')
    .map((node: NodeSingular) => ({
      id: node.id(),
      namespace: String(node.data('namespace')),
      position: { ...node.position() },
      width: node.outerWidth(),
      height: node.outerHeight(),
    }))
}

function applyPositions(cy: Core, positions: Positions): void {
  cy.batch(() => {
    for (const [id, point] of positions) cy.getElementById(id).position({ ...point })
  })
}

/** Runs dagre over the visible elements and resolves once it has settled. */
function runLayered(cy: Core): Promise<void> {
  return new Promise((resolve) => {
    const layout = cy.elements(':visible').layout({
      name: 'dagre',
      rankDir: 'TB',
      nodeSep: LAYOUT.nodeSep,
      rankSep: LAYOUT.rankSep,
      animate: false,
      fit: false,
    } as never)
    layout.one('layoutstop', () => resolve())
    layout.run()
  })
}

/**
 * Stretches the current layout to the window's aspect ratio and fits it.
 *
 * `base` is the layout before any stretch; stretching always starts there.
 */
export function reflow(cy: Core, base: Positions, animate = true): void {
  applyPositions(cy, base)
  // The graph is fitted with padding, so the shape to match is the window
  // minus that padding — matching the raw window would leave the long axis
  // short of the edges.
  const usableWidth = cy.width() - 2 * LAYOUT.fitPadding
  const usableHeight = cy.height() - 2 * LAYOUT.fitPadding
  const target = usableWidth > 0 && usableHeight > 0 ? usableWidth / usableHeight : 1
  applyPositions(
    cy,
    stretchToAspect(visibleNodes(cy), target, {
      tolerance: LAYOUT.tolerance,
      iterations: LAYOUT.iterations,
    }),
  )
  const visible = cy.elements(':visible')
  if (visible.empty()) return
  if (animate) {
    cy.animate({ fit: { eles: visible, padding: LAYOUT.fitPadding } }, { duration: LAYOUT.fitDuration })
  } else {
    cy.fit(visible, LAYOUT.fitPadding)
  }
}

/**
 * Lays the currently visible elements out for `plan` and fits them.
 *
 * @returns the pre-stretch positions, to be replayed by {@link reflow}.
 */
export async function applyLayout(cy: Core, plan: LayoutPlan): Promise<Positions> {
  const base = plan.kind === 'layered' ? await layeredBase(cy) : egoBase(cy, plan)
  reflow(cy, base)
  return base
}

async function layeredBase(cy: Core): Promise<Positions> {
  await runLayered(cy)
  return orderRanks(visibleNodes(cy), LAYOUT.gap)
}

function egoBase(cy: Core, plan: { focusId: string; namespaces: readonly string[] }): Positions {
  const nodes = visibleNodes(cy)
  const focused = nodes.find((node) => node.id === plan.focusId)
  if (!focused) return new Map(nodes.map((node) => [node.id, node.position]))

  const cyNode = cy.getElementById(plan.focusId)
  const parentIds = new Set(cyNode.incomers('node').map((node: NodeSingular) => node.id()))
  const childIds = new Set(cyNode.outgoers('node').map((node: NodeSingular) => node.id()))
  return egoRows(
    {
      parents: nodes.filter((node) => parentIds.has(node.id)),
      focused,
      children: nodes.filter((node) => childIds.has(node.id)),
    },
    { gap: LAYOUT.gap, rowGap: LAYOUT.rowGap, namespaces: plan.namespaces },
  )
}
