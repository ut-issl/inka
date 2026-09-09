/**
 * The node list: every node, grouped by namespace, groups and members both in
 * topological order — the same order the graph derives from.
 *
 * The list is the second focus surface: an entry focuses its node, a header
 * focuses its namespace. Under focus it mirrors the graph's relationships,
 * parents in one colour and children in another, with the target emphasized.
 */

import { useEffect, useRef } from 'react'

import type { Focus, FocusSets } from '../model/focus'
import { isNamespaceFocused } from '../model/focus'
import { nodeOf, type ArgumentView } from '../model/view'
import { colorOf, type NamespaceColors } from '../graph/palette'
import { MODE_COLORS } from '../graph/stylesheet'

interface Props {
  readonly view: ArgumentView
  readonly focus: Focus
  readonly sets: FocusSets
  readonly colors: NamespaceColors
  readonly onFocusNode: (id: string) => void
  readonly onFocusNamespace: (name: string) => void
}

/** The relationship an entry has to the current focus, as a class name. */
function relation(sets: FocusSets, id: string): string {
  if (sets.primary.has(id)) return 'is-focused'
  if (sets.parents.has(id)) return 'is-parent'
  if (sets.children.has(id)) return 'is-child'
  return ''
}

export function NodeList({ view, focus, sets, colors, onFocusNode, onFocusNamespace }: Props) {
  const list = useRef<HTMLDivElement>(null)

  // Keep the focused entry in sight when focus arrives from the graph.
  useEffect(() => {
    const target = list.current?.querySelector('.is-focused')
    target?.scrollIntoView({ block: 'nearest' })
  }, [focus])

  return (
    <div className="node-list" ref={list} aria-label="Nodes by namespace">
      {view.groups.map((group) => (
        <section key={group.name} className="node-list__group">
          <button
            type="button"
            className={`node-list__header ${isNamespaceFocused(focus, group.name) ? 'is-focused' : ''}`}
            style={{ borderLeftColor: colorOf(colors, group.name) }}
            aria-pressed={isNamespaceFocused(focus, group.name)}
            onClick={() => onFocusNamespace(group.name)}
          >
            {group.name}
          </button>
          {group.nodeIds.map((id) => {
            const node = nodeOf(view, id)
            return (
              <button
                key={id}
                type="button"
                className={`node-list__item ${relation(sets, id)}`}
                aria-pressed={sets.primary.has(id) && focus.kind === 'node'}
                onClick={() => onFocusNode(id)}
              >
                <span className="node-list__dot" style={{ backgroundColor: MODE_COLORS[node.mode] }} />
                <span className="node-list__label">{node.id}</span>
              </button>
            )
          })}
        </section>
      ))}
    </div>
  )
}
