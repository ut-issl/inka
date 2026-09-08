/**
 * The shared argument view: the one structure both the graph and the node list
 * read from, so they cannot disagree about content or ordering.
 *
 * Everything derived here is deterministic. The export lists nodes in sorted
 * block-ID order; that position is a node's {@link ArgumentNode.index}, and it
 * is the tie-break for the topological order, which in turn fixes the node
 * list's group order, each group's contents, and the namespace palette. The
 * same project therefore always renders the same graph and the same list.
 */

import {
  isEdgeData,
  type ArgumentElement,
  type Deadline,
  type DischargeMode,
  type EdgeData,
  type Evidence,
  type NodeData,
} from './elements'

/** One block, with its resolved neighbours. */
export interface ArgumentNode {
  readonly id: string
  readonly mode: DischargeMode
  readonly namespace: string
  /** Contract text, already interpolated by inka. Never drawn inside a node. */
  readonly description: string
  readonly tags: readonly string[]
  /** True on an objective root of the argument. */
  readonly root: boolean
  readonly evidence?: Evidence | undefined
  readonly deadline?: Deadline | undefined
  /** Position in the export, i.e. rank in sorted block-ID order. */
  readonly index: number
  /** Parents in sorted block-ID order. */
  readonly parents: readonly string[]
  /** Children in the parent's declaration order. */
  readonly children: readonly string[]
}

/** A namespace and its members, both in topological order. */
export interface NamespaceGroup {
  readonly name: string
  readonly nodeIds: readonly string[]
}

export interface ArgumentView {
  readonly nodes: ReadonlyMap<string, ArgumentNode>
  /** Every node id in topological order, ties broken by block-ID order. */
  readonly order: readonly string[]
  /** Namespace groups in the order their first member appears in {@link order}. */
  readonly groups: readonly NamespaceGroup[]
  readonly edges: readonly EdgeData[]
}

/** Raised when an export is well-formed but not a usable argument. */
export class ViewError extends Error {
  override readonly name = 'ViewError'
}

interface Draft {
  readonly data: NodeData
  readonly index: number
  readonly parents: string[]
  readonly children: string[]
}

/**
 * Builds the shared view from a validated export.
 *
 * @throws {ViewError} on a duplicate id, an edge naming an unknown node, or a
 * cycle — none of which a checked project can produce, so they are reported
 * rather than papered over.
 */
export function buildView(elements: readonly ArgumentElement[]): ArgumentView {
  const drafts = collectNodes(elements)
  const edges = collectEdges(elements, drafts)

  const nodes = new Map<string, ArgumentNode>()
  for (const [id, draft] of drafts) {
    draft.parents.sort((a, b) => index(drafts, a) - index(drafts, b))
    nodes.set(id, {
      ...draft.data,
      index: draft.index,
      parents: draft.parents,
      children: draft.children,
    })
  }

  const order = topologicalOrder(nodes)
  return { nodes, order, groups: groupByNamespace(nodes, order), edges }
}

function collectNodes(elements: readonly ArgumentElement[]): Map<string, Draft> {
  const drafts = new Map<string, Draft>()
  for (const element of elements) {
    if (isEdgeData(element.data)) continue
    const data = element.data
    if (drafts.has(data.id)) {
      throw new ViewError(`duplicate node id \`${data.id}\` in the export`)
    }
    drafts.set(data.id, { data, index: drafts.size, parents: [], children: [] })
  }
  return drafts
}

function collectEdges(
  elements: readonly ArgumentElement[],
  drafts: ReadonlyMap<string, Draft>,
): EdgeData[] {
  const edges: EdgeData[] = []
  for (const element of elements) {
    if (!isEdgeData(element.data)) continue
    const edge = element.data
    const source = drafts.get(edge.source)
    const target = drafts.get(edge.target)
    if (!source || !target) {
      const missing = source ? edge.target : edge.source
      throw new ViewError(`edge \`${edge.id}\` names unknown node \`${missing}\``)
    }
    source.children.push(edge.target)
    target.parents.push(edge.source)
    edges.push(edge)
  }
  return edges
}

function index(drafts: ReadonlyMap<string, Draft>, id: string): number {
  return drafts.get(id)?.index ?? 0
}

/**
 * Kahn's algorithm, always taking the ready node of lowest block-ID rank, so
 * the order depends on the argument alone and not on iteration accidents.
 */
function topologicalOrder(nodes: ReadonlyMap<string, ArgumentNode>): string[] {
  const remaining = new Map<string, number>()
  for (const node of nodes.values()) remaining.set(node.id, node.parents.length)

  const ready: ArgumentNode[] = []
  for (const node of nodes.values()) {
    if (node.parents.length === 0) insertByIndex(ready, node)
  }

  const order: string[] = []
  while (ready.length > 0) {
    // `ready` is kept descending, so the lowest-index node pops off the end.
    const node = ready.pop() as ArgumentNode
    order.push(node.id)
    for (const childId of node.children) {
      const left = (remaining.get(childId) as number) - 1
      remaining.set(childId, left)
      if (left === 0) insertByIndex(ready, nodes.get(childId) as ArgumentNode)
    }
  }

  if (order.length !== nodes.size) {
    throw new ViewError('the exported argument contains a cycle')
  }
  return order
}

/** Inserts into a list kept in descending index order. */
function insertByIndex(ready: ArgumentNode[], node: ArgumentNode): void {
  let low = 0
  let high = ready.length
  while (low < high) {
    const mid = (low + high) >> 1
    if ((ready[mid] as ArgumentNode).index > node.index) low = mid + 1
    else high = mid
  }
  ready.splice(low, 0, node)
}

/** Groups by namespace, keeping both the groups and their members topological. */
function groupByNamespace(
  nodes: ReadonlyMap<string, ArgumentNode>,
  order: readonly string[],
): NamespaceGroup[] {
  const members = new Map<string, string[]>()
  for (const id of order) {
    const { namespace } = nodes.get(id) as ArgumentNode
    const group = members.get(namespace)
    if (group) group.push(id)
    else members.set(namespace, [id])
  }
  return [...members].map(([name, nodeIds]) => ({ name, nodeIds }))
}

/** Namespace names in group order — the order the palette and lists follow. */
export function namespacesOf(view: ArgumentView): string[] {
  return view.groups.map((group) => group.name)
}

/** Looks a node up, failing loudly rather than returning a partial UI. */
export function nodeOf(view: ArgumentView, id: string): ArgumentNode {
  const node = view.nodes.get(id)
  if (!node) throw new ViewError(`unknown node \`${id}\``)
  return node
}

/** The argument's objective roots, in topological order. */
export function rootsOf(view: ArgumentView): ArgumentNode[] {
  return view.order.map((id) => nodeOf(view, id)).filter((node) => node.root)
}
