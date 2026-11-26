/**
 * Where the cards go.
 *
 * Layout is a function of the visible cards, their measured sizes and the
 * edges between them, and of nothing else: the same input always gives the
 * same positions. The arithmetic is the ELK layered algorithm's — a top-down
 * layered drawing with roots at the top and every edge flowing downward —
 * and this module only states the question and reads back the answer.
 */

import ELK, { type ElkNode } from 'elkjs/lib/elk.bundled.js'

export interface CardSize {
  readonly id: string
  readonly width: number
  readonly height: number
}

export interface CardEdge {
  readonly source: string
  readonly target: string
}

export interface Point {
  readonly x: number
  readonly y: number
}

/** Presentation parameters: free to tune, never an invariant. */
export const SPACING = {
  /** Horizontal gap between neighbouring cards of one layer. */
  between: 28,
  /** Vertical gap between one layer and the next. */
  layers: 72,
} as const

const OPTIONS = {
  'elk.algorithm': 'layered',
  'elk.direction': 'DOWN',
  'elk.spacing.nodeNode': String(SPACING.between),
  'elk.layered.spacing.nodeNodeBetweenLayers': String(SPACING.layers),
  // Keep the order the cards and edges are given in — the roots' order and
  // each parent's declaration order — wherever crossings allow it, so the
  // drawing is stable and reads the way the argument is written.
  'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
  // Parts of the graph that share no edge are drawn side by side.
  'elk.separateConnectedComponents': 'true',
  'elk.spacing.componentComponent': String(SPACING.layers),
} as const

const elk = new ELK()

/**
 * Lays the cards out and returns the top-left corner of each.
 *
 * Edges naming a card that is not given are ignored: an edge is drawn only
 * between two visible cards.
 */
export async function layoutCards(
  cards: readonly CardSize[],
  edges: readonly CardEdge[],
): Promise<Map<string, Point>> {
  const known = new Set(cards.map((card) => card.id))
  const graph: ElkNode = {
    id: 'argument',
    layoutOptions: OPTIONS,
    children: cards.map((card) => ({ id: card.id, width: card.width, height: card.height })),
    edges: edges
      .filter((edge) => known.has(edge.source) && known.has(edge.target))
      .map((edge) => ({ id: `${edge.source}->${edge.target}`, sources: [edge.source], targets: [edge.target] })),
  }
  const laid = await elk.layout(graph)
  return new Map((laid.children ?? []).map((child) => [child.id, { x: child.x ?? 0, y: child.y ?? 0 }]))
}
