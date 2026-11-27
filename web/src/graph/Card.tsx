/**
 * One node of the graph: a card that carries its mode, its id, its status and
 * its description, rendered as in the local view.
 *
 * A long description is clipped here — the node's local view shows it whole.
 * A node with children can be expanded or collapsed on its own, and while
 * collapsed it says how many nodes lie below it and how many of those are
 * blamed, so nothing is hidden silently and what needs attention can be found
 * without expanding everything.
 *
 * Selecting a card — a click anywhere on it, or its id activated from the
 * keyboard — opens the node's local view beside the graph. A card records
 * nothing: it offers no decision.
 */

import { Handle, Position, type Node, type NodeProps } from '@xyflow/react'

import { Markdown } from '../components/Markdown'
import { StatusBadge } from '../components/StatusBadge'
import type { NodeData } from '../generated/NodeData'
import { neutralize } from '../text/neutralize'

export interface CardData extends Record<string, unknown> {
  readonly node: NodeData
  readonly expanded: boolean
  /** Whether no objective reaches this root. */
  readonly unreachable: boolean
  /** Whether this node's local view is open beside the graph. */
  readonly selected: boolean
  readonly onToggle: (id: string) => void
  readonly onSelect: (id: string) => void
}

export type CardNode = Node<CardData, 'card'>

export function Card({ data }: NodeProps<CardNode>) {
  const { node, expanded, unreachable, selected, onToggle, onSelect } = data
  const hasChildren = node.children.length > 0
  return (
    <div
      className={`card card--${node.mode} card--${node.status.state} ${selected ? 'is-selected' : ''}`}
      data-id={node.id}
      aria-current={selected ? 'true' : undefined}
    >
      {node.parents.length > 0 ? <Handle type="target" position={Position.Top} isConnectable={false} /> : null}
      <header className="card__header">
        <span className="card__mode">{node.mode}</span>
        <StatusBadge status={node.status} />
      </header>
      <p className="card__name">
        {/* A real control, so the keyboard opens the local view as a click does. */}
        <button
          type="button"
          className="card__open nodrag nopan"
          aria-label={`Open the local view of ${node.id}`}
          onClick={(event) => {
            event.stopPropagation()
            onSelect(node.id)
          }}
        >
          <span className="card__id">{neutralize(node.id)}</span>
        </button>
        <span className="card__namespace">{neutralize(node.namespace)}</span>
      </p>
      {unreachable ? <p className="card__unreachable">unreachable from any objective</p> : null}
      <div className="card__description">
        <Markdown text={node.local.description} />
      </div>
      {hasChildren ? (
        <button
          type="button"
          className="card__toggle nodrag nopan"
          aria-expanded={expanded}
          onClick={(event) => {
            event.stopPropagation()
            onToggle(node.id)
          }}
        >
          {expanded ? 'Collapse' : below(node)}
        </button>
      ) : null}
      {hasChildren ? <Handle type="source" position={Position.Bottom} isConnectable={false} /> : null}
    </div>
  )
}

/** What a collapsed node says about what it hides. */
function below(node: NodeData): string {
  const nodes = `${node.below} node${node.below === 1 ? '' : 's'} below`
  return node.blamed_below > 0 ? `Expand · ${nodes} · ${node.blamed_below} blamed` : `Expand · ${nodes}`
}
