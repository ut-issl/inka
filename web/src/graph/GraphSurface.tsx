/**
 * The graph surface: the whole argument as cards, explored from its roots
 * down by expanding and collapsing.
 *
 * It opens collapsed — the roots and their direct children — and the whole
 * graph can be expanded, and collapsed again, in one action.
 *
 * Selecting a card opens that node's local view beside the graph, which stays
 * in place: the local view is the node's close-up, and there is no separate
 * graph of a node's neighbours. A node opened from elsewhere — the sidebar, a
 * link in a local view — is revealed by expanding a path from a root down to
 * it. Esc, or a tap on the empty background, clears the selection.
 *
 * Nothing here records anything; a queued node can be opened in the review
 * surface, which is where decisions are taken.
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'

import { LocalView } from '../components/LocalView'
import { NodeReference } from '../components/NodeReference'
import { Sidebar } from '../components/Sidebar'
import type { NodeData } from '../generated/NodeData'
import type { Snapshot } from '../generated/Snapshot'
import { DISCHARGE_MODES } from '../model/modes'
import { graphPath, reviewPath } from '../paths'
import { CardGraph } from './CardGraph'
import { allExpanded, graphRoots, revealing, toggled, visibleNodes } from './visibility'

interface Props {
  readonly snapshot: Snapshot
  readonly nodes: ReadonlyMap<string, NodeData>
}

export function GraphSurface({ snapshot, nodes }: Props) {
  const params = useParams()
  const navigate = useNavigate()
  const roots = useMemo(() => graphRoots(snapshot), [snapshot])
  const unreachable = useMemo(() => new Set(snapshot.unreachable_roots), [snapshot])
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set(roots))

  const selected = params.id === undefined ? undefined : nodes.get(params.id)
  const selectedId = selected?.id

  // A selected node is on screen: open a path down to it, closing nothing.
  useEffect(() => {
    if (selectedId !== undefined) setExpanded((current) => revealing(nodes, current, selectedId))
  }, [nodes, selectedId])

  const visible = useMemo(() => visibleNodes(nodes, roots, expanded), [nodes, roots, expanded])
  const toggle = useCallback((id: string) => setExpanded((current) => toggled(current, id)), [])
  const select = useCallback((id: string) => void navigate(graphPath(id)), [navigate])
  const clear = useCallback(() => void navigate('/graph'), [navigate])

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') clear()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [clear])

  return (
    <div className={`graph-surface ${selected ? 'graph-surface--selected' : ''}`}>
      <Sidebar snapshot={snapshot} nodes={nodes} surface="graph" currentId={selectedId} />
      <div className="graph-surface__main">
        <div className="graph-surface__bar">
          <button type="button" className="button" onClick={() => setExpanded(allExpanded(nodes))}>
            Expand all
          </button>
          <button
            type="button"
            className="button"
            onClick={() => setExpanded(selectedId === undefined ? new Set(roots) : revealing(nodes, new Set(roots), selectedId))}
          >
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
        <CardGraph
          nodes={nodes}
          visible={visible}
          expanded={expanded}
          unreachable={unreachable}
          selectedId={selectedId}
          onToggle={toggle}
          onSelect={select}
          onClear={clear}
        />
      </div>
      {selected ? (
        <section className="graph-surface__panel" aria-label="Selected node">
          <LocalView
            node={selected}
            reference={(id) => <NodeReference node={nodes.get(id)} id={id} to={graphPath(id)} />}
            actions={
              <>
                {selected.review ? (
                  <Link className="button" to={reviewPath(selected.id)}>
                    Open in review
                  </Link>
                ) : null}
                <button type="button" className="button" onClick={clear}>
                  Close
                </button>
              </>
            }
          />
        </section>
      ) : null}
    </div>
  )
}
