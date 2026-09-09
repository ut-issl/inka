/**
 * Focus state and the sets every surface derives from it.
 *
 * There are exactly two focus targets — a single node and a namespace — and
 * "one hop" always means direct parents and children. Both the graph and the
 * node list read the same {@link FocusSets}, so a highlight in the list and a
 * visible node in the graph can never disagree.
 */

import { nodeOf, type ArgumentView } from './view'

export type Focus =
  | { readonly kind: 'whole' }
  | { readonly kind: 'node'; readonly id: string }
  | { readonly kind: 'namespace'; readonly name: string }

/** The unfocused whole view. */
export const WHOLE_VIEW: Focus = { kind: 'whole' }

export interface FocusSets {
  /** Nodes the graph shows; `null` means every node (the whole view). */
  readonly visible: ReadonlySet<string> | null
  /** The focus target itself: one node, or a namespace's members. */
  readonly primary: ReadonlySet<string>
  /** One hop upward from the target. */
  readonly parents: ReadonlySet<string>
  /** One hop downward from the target. */
  readonly children: ReadonlySet<string>
}

const EMPTY: ReadonlySet<string> = new Set()

/** Resolves a focus against the shared view. Pure; returns fresh sets. */
export function resolveFocus(view: ArgumentView, focus: Focus): FocusSets {
  switch (focus.kind) {
    case 'whole':
      return { visible: null, primary: EMPTY, parents: EMPTY, children: EMPTY }
    case 'node':
      return nodeFocus(view, focus.id)
    case 'namespace':
      return namespaceFocus(view, focus.name)
  }
}

function nodeFocus(view: ArgumentView, id: string): FocusSets {
  const node = nodeOf(view, id)
  const parents = new Set(node.parents)
  const children = new Set(node.children)
  return {
    visible: new Set([id, ...parents, ...children]),
    primary: new Set([id]),
    parents,
    children,
  }
}

function namespaceFocus(view: ArgumentView, name: string): FocusSets {
  const members = new Set(
    view.order.filter((id) => nodeOf(view, id).namespace === name),
  )
  const parents = new Set<string>()
  const children = new Set<string>()
  for (const id of members) {
    const node = nodeOf(view, id)
    // A neighbour inside the namespace is a member, not a one-hop outsider.
    for (const parent of node.parents) {
      if (!members.has(parent)) parents.add(parent)
    }
    for (const child of node.children) {
      if (!members.has(child)) children.add(child)
    }
  }
  // A node sitting both above and below the namespace reads as a parent.
  for (const parent of parents) children.delete(parent)

  return {
    visible: new Set([...members, ...parents, ...children]),
    primary: members,
    parents,
    children,
  }
}

/** Whether the focus targets exactly this node. */
export function isNodeFocused(focus: Focus, id: string): boolean {
  return focus.kind === 'node' && focus.id === id
}

/** Whether the focus targets exactly this namespace. */
export function isNamespaceFocused(focus: Focus, name: string): boolean {
  return focus.kind === 'namespace' && focus.name === name
}
