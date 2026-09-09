/**
 * Layout arithmetic, expressed as pure functions over node boxes.
 *
 * dagre supplies the ranking (which nodes share a row, and in which order the
 * rows flow); everything the Web UI invariants add on top lives here:
 *
 * - {@link orderRanks} reorders each rank so same-namespace nodes are adjacent
 *   in one namespace order shared by every rank, and re-spaces the rank from
 *   the nodes' own widths so a reorder never overlaps two labels.
 * - {@link egoRows} lays the node-focus rows out: parents above, the focused
 *   node in the middle, children below.
 * - {@link stretchToAspect} matches the laid-out graph's aspect ratio to the
 *   graph window by stretching the short axis only — never compressing, so
 *   nodes that were apart stay apart.
 *
 * Keeping them free of cytoscape means the invariants are testable without a
 * browser; {@link ./runLayout} is the thin adapter that feeds them.
 */

export interface Point {
  readonly x: number
  readonly y: number
}

/** A node as the layout sees it: where it sits and how much room it takes. */
export interface LayoutNode {
  readonly id: string
  readonly namespace: string
  readonly position: Point
  readonly width: number
  readonly height: number
}

export type Positions = ReadonlyMap<string, Point>

export interface Box {
  readonly x1: number
  readonly y1: number
  readonly x2: number
  readonly y2: number
}

/** Below this extent an axis is treated as having no spread to stretch. */
const DEGENERATE = 1e-6

/** Bounding box of the nodes including their own width and height. */
export function boundingBox(nodes: readonly LayoutNode[]): Box | null {
  if (nodes.length === 0) return null
  let x1 = Infinity
  let y1 = Infinity
  let x2 = -Infinity
  let y2 = -Infinity
  for (const node of nodes) {
    x1 = Math.min(x1, node.position.x - node.width / 2)
    y1 = Math.min(y1, node.position.y - node.height / 2)
    x2 = Math.max(x2, node.position.x + node.width / 2)
    y2 = Math.max(y2, node.position.y + node.height / 2)
  }
  return { x1, y1, x2, y2 }
}

/**
 * Groups nodes into ranks by their y, ordering the ranks top to bottom.
 *
 * A rank is a set of nodes a layered layout put on one row, so equal y is the
 * membership test; y is rounded because a layered pass may leave sub-pixel
 * differences within a row.
 */
export function ranksOf(nodes: readonly LayoutNode[]): LayoutNode[][] {
  const byRow = new Map<number, LayoutNode[]>()
  for (const node of nodes) {
    const row = Math.round(node.position.y)
    const rank = byRow.get(row)
    if (rank) rank.push(node)
    else byRow.set(row, [node])
  }
  return [...byRow.entries()].sort(([a], [b]) => a - b).map(([, rank]) => rank)
}

/**
 * Ranks the namespaces by their mean x, so the order follows the crossing
 * minimisation the layered pass already did. Ties break on the name, keeping
 * the result independent of iteration order.
 */
export function namespaceOrder(nodes: readonly LayoutNode[]): string[] {
  const totals = new Map<string, { sum: number; count: number }>()
  for (const node of nodes) {
    const total = totals.get(node.namespace) ?? { sum: 0, count: 0 }
    total.sum += node.position.x
    total.count += 1
    totals.set(node.namespace, total)
  }
  return [...totals.entries()]
    .sort(([nameA, a], [nameB, b]) => a.sum / a.count - b.sum / b.count || nameA.localeCompare(nameB))
    .map(([name]) => name)
}

/**
 * Places one rank left to right at `y`, centred on x = 0, leaving `gap`
 * between neighbouring boxes.
 */
export function placeRow(nodes: readonly LayoutNode[], y: number, gap: number): Positions {
  const positions = new Map<string, Point>()
  const total =
    nodes.reduce((sum, node) => sum + node.width, 0) + gap * Math.max(0, nodes.length - 1)
  let cursor = -total / 2
  for (const node of nodes) {
    positions.set(node.id, { x: cursor + node.width / 2, y })
    cursor += node.width + gap
  }
  return positions
}

/**
 * Reorders every rank so same-namespace nodes are adjacent, in one namespace
 * order shared across ranks — namespaces then read as near-vertical bands.
 * Ranks keep their y, so the layered flow is untouched.
 */
export function orderRanks(nodes: readonly LayoutNode[], gap: number): Positions {
  const order = new Map(namespaceOrder(nodes).map((name, rank) => [name, rank]))
  const positions = new Map<string, Point>()
  for (const rank of ranksOf(nodes)) {
    const sorted = [...rank].sort(
      (a, b) =>
        (order.get(a.namespace) as number) - (order.get(b.namespace) as number) ||
        a.position.x - b.position.x ||
        a.id.localeCompare(b.id),
    )
    const y = (rank[0] as LayoutNode).position.y
    for (const [id, point] of placeRow(sorted, y, gap)) positions.set(id, point)
  }
  return positions
}

export interface EgoRows {
  readonly parents: readonly LayoutNode[]
  readonly focused: LayoutNode
  readonly children: readonly LayoutNode[]
}

export interface EgoOptions {
  /** Gap between neighbouring nodes in a row. */
  readonly gap: number
  /** Vertical distance between the three rows. */
  readonly rowGap: number
  /** Namespace order the rows sort by, shared with the rest of the UI. */
  readonly namespaces: readonly string[]
}

/**
 * The node-focus layout: direct parents on top, the focused node in the
 * middle, direct children below. Rows are ordered by the same namespace order
 * as everywhere else, so a node keeps its horizontal band on focus.
 */
export function egoRows(rows: EgoRows, options: EgoOptions): Positions {
  const order = new Map(options.namespaces.map((name, rank) => [name, rank]))
  const rank = (node: LayoutNode) => order.get(node.namespace) ?? options.namespaces.length
  const sort = (nodes: readonly LayoutNode[]) =>
    [...nodes].sort((a, b) => rank(a) - rank(b) || a.id.localeCompare(b.id))

  const positions = new Map<string, Point>()
  const place = (nodes: readonly LayoutNode[], y: number) => {
    for (const [id, point] of placeRow(nodes, y, options.gap)) positions.set(id, point)
  }
  place(sort(rows.parents), 0)
  place([rows.focused], options.rowGap)
  place(sort(rows.children), options.rowGap * 2)
  return positions
}

export interface StretchOptions {
  /** Relative aspect-ratio error accepted as a match. */
  readonly tolerance: number
  /** Cap on the passes; node sizes leave a residual after each stretch. */
  readonly iterations: number
}

/**
 * Stretches the layout about its centre until its bounding box has the target
 * aspect ratio.
 *
 * Only the short axis ever moves: spreading nodes apart cannot make them
 * overlap, whereas compressing the long axis could. Node sizes do not scale,
 * so one pass undershoots and the stretch repeats until it converges. An axis
 * with no spread (a single row, a single node) is left alone — there is
 * nothing to stretch, and the ratio simply cannot be matched.
 */
export function stretchToAspect(
  nodes: readonly LayoutNode[],
  targetAspect: number,
  options: StretchOptions,
): Positions {
  let current = new Map(nodes.map((node) => [node.id, node.position]))
  if (nodes.length < 2 || !Number.isFinite(targetAspect) || targetAspect <= 0) return current

  for (let pass = 0; pass < options.iterations; pass += 1) {
    const moved = nodes.map((node) => ({ ...node, position: current.get(node.id) as Point }))
    const box = boundingBox(moved) as Box
    const width = box.x2 - box.x1
    const height = box.y2 - box.y1
    if (width < DEGENERATE || height < DEGENERATE) return current

    const aspect = width / height
    if (Math.abs(aspect - targetAspect) / targetAspect < options.tolerance) return current

    const scaleX = aspect < targetAspect ? targetAspect / aspect : 1
    const scaleY = aspect > targetAspect ? aspect / targetAspect : 1
    const centreX = (box.x1 + box.x2) / 2
    const centreY = (box.y1 + box.y2) / 2
    const spreadX = width <= (widestNode(nodes) ?? 0) ? 1 : scaleX
    const spreadY = height <= (tallestNode(nodes) ?? 0) ? 1 : scaleY
    if (spreadX === 1 && spreadY === 1) return current

    current = new Map(
      moved.map((node) => [
        node.id,
        {
          x: centreX + (node.position.x - centreX) * spreadX,
          y: centreY + (node.position.y - centreY) * spreadY,
        },
      ]),
    )
  }
  return current
}

/** Width of the widest node — the floor a single column's spread cannot beat. */
function widestNode(nodes: readonly LayoutNode[]): number | null {
  return nodes.length === 0 ? null : Math.max(...nodes.map((node) => node.width))
}

/** Height of the tallest node — the floor a single row's spread cannot beat. */
function tallestNode(nodes: readonly LayoutNode[]): number | null {
  return nodes.length === 0 ? null : Math.max(...nodes.map((node) => node.height))
}
