/**
 * Which nodes the graph shows, given what the viewer has expanded.
 *
 * This is view state, not meaning: the nodes, their children and the roots
 * arrive from the server, and the only thing decided here is which of them
 * are on screen. One rule decides it — a node is visible exactly when it is a
 * root or a child of an expanded, visible node — so the answer depends on the
 * set of expanded nodes alone, never on the order they were expanded in.
 */

import type { NodeData } from '../generated/NodeData'
import type { Snapshot } from '../generated/Snapshot'

type Nodes = ReadonlyMap<string, NodeData>

/**
 * Where the drawing starts: the objectives the argument rests on, then the
 * topmost of the nodes no objective reaches. Every node lies below one.
 */
export function graphRoots(snapshot: Pick<Snapshot, 'roots' | 'unreachable_roots'>): string[] {
  return [...snapshot.roots, ...snapshot.unreachable_roots]
}

/** The visible nodes, parents before children, each once. */
export function visibleNodes(nodes: Nodes, roots: readonly string[], expanded: ReadonlySet<string>): string[] {
  const visible = new Set<string>()
  const pending = [...roots]
  for (let next = pending.shift(); next !== undefined; next = pending.shift()) {
    const node = nodes.get(next)
    if (!node || visible.has(next)) continue
    visible.add(next)
    if (expanded.has(next)) pending.push(...node.children)
  }
  return [...visible]
}

/** Every node that has children: the set that shows the whole graph. */
export function allExpanded(nodes: Nodes): Set<string> {
  return new Set([...nodes.values()].filter((node) => node.children.length > 0).map((node) => node.id))
}

/**
 * The expanded set that also shows `id`: every node on one path from a root
 * down to its parent is expanded, and nothing already open is closed. The
 * path climbs through each node's first parent, so the same node is always
 * revealed the same way.
 */
export function revealing(nodes: Nodes, expanded: ReadonlySet<string>, id: string): Set<string> {
  const opened = new Set(expanded)
  const seen = new Set<string>()
  let current = nodes.get(id)
  while (current && !seen.has(current.id)) {
    seen.add(current.id)
    const parent = current.parents[0]
    if (parent === undefined) break
    opened.add(parent)
    current = nodes.get(parent)
  }
  return opened
}

/** `expanded` with `id` flipped. */
export function toggled(expanded: ReadonlySet<string>, id: string): Set<string> {
  const next = new Set(expanded)
  if (!next.delete(id)) next.add(id)
  return next
}
