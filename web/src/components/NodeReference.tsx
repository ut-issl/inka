/**
 * A reference to a node: its id as a link, with a preview on hover or focus.
 *
 * The preview is supplementary — the node's mode, id, namespace, status and
 * description — and is never the only way to read anything: the link leads to
 * the node's own local view, which shows all of it and more. A preview offers
 * no decision. Opening, closing and placing it are the library's.
 */

import * as HoverCard from '@radix-ui/react-hover-card'
import type { ReactNode } from 'react'
import { Link } from 'react-router'

import type { NodeData } from '../generated/NodeData'
import { neutralize } from '../text/neutralize'
import { Markdown } from './Markdown'
import { StatusBadge } from './StatusBadge'

interface Props {
  /** The node referred to; `undefined` when the argument does not define it. */
  readonly node: NodeData | undefined
  readonly id: string
  /** Where the reference leads. */
  readonly to: string
  readonly className?: string
  /** What the link shows; the id when not given. */
  readonly children?: ReactNode
  readonly current?: boolean
}

export function NodeReference({ node, id, to, className = 'reference', children, current = false }: Props) {
  const link = (
    <Link className={className} to={to} aria-current={current ? 'true' : undefined}>
      {children ?? neutralize(id)}
    </Link>
  )
  if (!node) return link
  return (
    <HoverCard.Root openDelay={300} closeDelay={80}>
      <HoverCard.Trigger asChild>{link}</HoverCard.Trigger>
      <HoverCard.Portal>
        <HoverCard.Content className="preview" side="right" align="start" sideOffset={10} collisionPadding={12}>
          <NodePreview node={node} />
        </HoverCard.Content>
      </HoverCard.Portal>
    </HoverCard.Root>
  )
}

/** What a preview shows of a node. */
export function NodePreview({ node }: { node: NodeData }) {
  return (
    <div className="preview__body" role="note" aria-label={`Preview of ${node.id}`}>
      <p className="preview__target">
        <span className="preview__mode">{node.mode}:</span>
        <span className="preview__id">{neutralize(node.id)}</span>
        <StatusBadge status={node.status} />
      </p>
      <p className="preview__namespace">{neutralize(node.namespace)}</p>
      <Markdown text={node.local.description} />
    </div>
  )
}
