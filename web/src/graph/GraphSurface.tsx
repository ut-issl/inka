/**
 * The graph surface: the whole argument as cards, explored from its roots
 * down by expanding and collapsing.
 *
 * It opens collapsed — the roots and their direct children — and the whole
 * graph can be expanded, and collapsed again, in one action.
 */

import { useCallback, useMemo, useState } from 'react'

import type { NodeData } from '../generated/NodeData'
import type { Snapshot } from '../generated/Snapshot'
import { DISCHARGE_MODES } from '../model/modes'
import { CardGraph } from './CardGraph'
import { allExpanded, graphRoots, toggled, visibleNodes } from './visibility'

interface Props {
  readonly snapshot: Snapshot
  readonly nodes: ReadonlyMap<string, NodeData>
}

export function GraphSurface({ snapshot, nodes }: Props) {
  const roots = useMemo(() => graphRoots(snapshot), [snapshot])
  const unreachable = useMemo(() => new Set(snapshot.unreachable_roots), [snapshot])
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set(roots))

  const visible = useMemo(() => visibleNodes(nodes, roots, expanded), [nodes, roots, expanded])
  const toggle = useCallback((id: string) => setExpanded((current) => toggled(current, id)), [])

  return (
    <div className="graph-surface">
      <div className="graph-surface__bar">
        <button type="button" className="button" onClick={() => setExpanded(allExpanded(nodes))}>
          Expand all
        </button>
        <button type="button" className="button" onClick={() => setExpanded(new Set(roots))}>
          Collapse all
        </button>
        <span className="graph-surface__count">
          {visible.length} of {nodes.size} nodes shown
        </span>
        <ul className="legend" aria-label="Discharge modes">
          {DISCHARGE_MODES.map((mode) => (
            <li key={mode} className={`legend__mode legend__mode--${mode}`}>
              {mode}
            </li>
          ))}
        </ul>
      </div>
      <CardGraph nodes={nodes} visible={visible} expanded={expanded} unreachable={unreachable} onToggle={toggle} />
    </div>
  )
}
