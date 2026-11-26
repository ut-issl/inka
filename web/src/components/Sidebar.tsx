/**
 * The sidebar both surfaces share: the review queue, every node of the
 * argument, and a search over them.
 *
 * It is supplementary navigation. The queue is the one `inka review --list`
 * prints and the nodes are in the server's parents-first order; nothing here
 * decides what is listed or in what order. The search narrows the list to the
 * nodes whose id or description contains what was typed — a filter over text
 * already on the page, not a judgement about the argument.
 *
 * An entry shows the node's mode and status, and choosing one opens the node
 * on the surface the sidebar is on. No entry records anything.
 */

import { useId, useState } from 'react'

import type { NodeData } from '../generated/NodeData'
import type { Snapshot } from '../generated/Snapshot'
import { groupHeading, reviewDetail } from '../model/wording'
import { nodePath, type Surface } from '../paths'
import { neutralize } from '../text/neutralize'
import { NodeReference } from './NodeReference'
import { StatusBadge } from './StatusBadge'

interface Props {
  readonly snapshot: Snapshot
  readonly nodes: ReadonlyMap<string, NodeData>
  readonly surface: Surface
  /** The node the surface is showing, if any. */
  readonly currentId: string | undefined
}

export function Sidebar({ snapshot, nodes, surface, currentId }: Props) {
  const [query, setQuery] = useState('')
  const searchId = useId()
  const wanted = query.trim().toLowerCase()

  return (
    <aside className="sidebar" aria-label="Sidebar">
      <div className="sidebar__search">
        <label className="visually-hidden" htmlFor={searchId}>
          Search nodes by id or description
        </label>
        <input
          id={searchId}
          type="search"
          className="sidebar__input"
          placeholder="Search id or description"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
      </div>
      {wanted === '' ? (
        <>
          <Queue snapshot={snapshot} nodes={nodes} surface={surface} currentId={currentId} />
          <NodeList
            label="All nodes"
            heading={`All nodes (${snapshot.order.length})`}
            ids={snapshot.order}
            nodes={nodes}
            surface={surface}
            currentId={currentId}
          />
        </>
      ) : (
        <Results wanted={wanted} snapshot={snapshot} nodes={nodes} surface={surface} currentId={currentId} />
      )}
    </aside>
  )
}

function Results({ wanted, snapshot, nodes, surface, currentId }: Props & { wanted: string }) {
  const found = snapshot.order.filter((id) => {
    const node = nodes.get(id)
    return node !== undefined && (id.toLowerCase().includes(wanted) || node.local.description.toLowerCase().includes(wanted))
  })
  return (
    <NodeList
      label="Search results"
      heading={found.length === 0 ? 'No node matches' : `${found.length} of ${snapshot.order.length} nodes match`}
      ids={found}
      nodes={nodes}
      surface={surface}
      currentId={currentId}
    />
  )
}

/** The queue as `inka review --list` prints it: one section per state. */
function Queue({ snapshot, nodes, surface, currentId }: Props) {
  const groups = snapshot.queue.groups
  const total = groups
    .filter((group) => group.state !== 'blocked')
    .reduce((sum, group) => sum + group.targets.length, 0)
  return (
    <nav className="queue" aria-label="Review queue">
      <p className="queue__summary">
        {total === 0 ? 'nothing needs review' : `${total} target${total === 1 ? '' : 's'} need${total === 1 ? 's' : ''} review`}
      </p>
      {groups.map((group) => (
        <section key={group.state} className="queue__group">
          <h3 className={`queue__heading queue__heading--${group.state}`}>
            {groupHeading(group.state)} ({group.targets.length})
          </h3>
          <ul className="queue__targets">
            {group.targets.map((id) => {
              const node = nodes.get(id)
              if (!node) return null
              const detail = node.review ? reviewDetail(node.review) : null
              return (
                <li key={id}>
                  <NodeReference
                    node={node}
                    id={id}
                    to={nodePath(surface, id)}
                    className={`queue__target ${id === currentId ? 'is-current' : ''}`}
                    current={id === currentId}
                  >
                    <span className="queue__mode">{node.mode}:</span>
                    <span className="queue__id">{neutralize(id)}</span>
                    {detail ? <span className="queue__detail">{neutralize(detail)}</span> : null}
                  </NodeReference>
                </li>
              )
            })}
          </ul>
        </section>
      ))}
    </nav>
  )
}

interface ListProps {
  readonly label: string
  readonly heading: string
  readonly ids: readonly string[]
  readonly nodes: ReadonlyMap<string, NodeData>
  readonly surface: Surface
  readonly currentId: string | undefined
}

/** A list of nodes, each with its mode, its namespace and its status. */
function NodeList({ label, heading, ids, nodes, surface, currentId }: ListProps) {
  return (
    <nav className="nodes" aria-label={label}>
      <h3 className="nodes__heading">{heading}</h3>
      <ul className="queue__targets">
        {ids.map((id) => {
          const node = nodes.get(id)
          if (!node) return null
          return (
            <li key={id}>
              <NodeReference
                node={node}
                id={id}
                to={nodePath(surface, id)}
                className={`queue__target nodes__entry ${id === currentId ? 'is-current' : ''}`}
                current={id === currentId}
              >
                <span className="nodes__name">
                  <span className="queue__mode">{node.mode}:</span>
                  <span className="queue__id">{neutralize(id)}</span>
                  <span className="nodes__namespace">{neutralize(node.namespace)}</span>
                </span>
                <StatusBadge status={node.status} />
              </NodeReference>
            </li>
          )
        })}
      </ul>
    </nav>
  )
}
