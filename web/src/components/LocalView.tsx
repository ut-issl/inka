/**
 * A node's local view: what an approval of the node binds to, and so what a
 * reviewer reads.
 *
 * The content and its order are the server's (`LocalContent`), the same that
 * `inka review` prints: the comment on the block, the description, the
 * evidence reference, the direct children — each with the comment on its
 * entry — and the deadline, every property under the comment attached to it.
 *
 * Nothing deeper is shown and nothing expands in place: a child appears as its
 * description only, and reading below it means opening that child's own local
 * view. Everything here is on screen without a hover or a click; the one
 * exception is the exact spacing of a description, read in its source.
 *
 * A child's card also says where the child stands — its status, and when it
 * is blamed, why — as `inka review` does in the child's frame. The queue
 * offers a parent whatever state its children are in, and a blocked child is
 * never offered at all, so without this a reviewer could not tell why a
 * parent turned up before the claims under it. The status is not contract
 * content: no approval binds to it, and it is set apart inside the card.
 *
 * Below the contract content, and set apart from it, comes what only helps
 * to move around: the node's parents, and whatever the surface adds.
 */

import type { ReactNode } from 'react'

import type { NodeData } from '../generated/NodeData'
import { childStatusDetail, reviewDetail, statusDetail } from '../model/wording'
import { neutralize } from '../text/neutralize'
import { Description } from './Description'
import { ReviewBadge, StatusBadge } from './StatusBadge'

interface Props {
  readonly node: NodeData
  /** Every node by id, where a child's status is read; a child not found is shown without one. */
  readonly nodes?: ReadonlyMap<string, NodeData>
  /** Draws a reference to another node; a plain id unless the surface links it. */
  readonly reference?: (id: string) => ReactNode
  /** Further navigation the surface offers from this node, set apart with the parents. */
  readonly actions?: ReactNode
}

export function LocalView({ node, nodes, reference = plainReference, actions }: Props) {
  const local = node.local
  const review = node.review
  return (
    <article className={`local local--${review?.state ?? 'settled'}`} aria-labelledby={`local-${node.id}`}>
      <header className="local__header">
        <h2 id={`local-${node.id}`} className="local__target">
          <span className="local__mode">{node.mode}:</span>
          <span className="local__id">{neutralize(node.id)}</span>
        </h2>
        <StatusBadge status={node.status} />
        {review ? <ReviewBadge state={review} /> : null}
        <span className="local__namespace">{neutralize(node.namespace)}</span>
      </header>
      <ul className="local__details">
        {statusDetail(node.status).map((detail) => (
          <li key={detail}>{neutralize(detail)}</li>
        ))}
        {review && reviewDetail(review) ? <li>{neutralize(reviewDetail(review) as string)}</li> : null}
      </ul>

      <section className="local__contract" aria-label="Contract content">
        <Comment text={local.comment} />
        <div className="local__subject">
          <Description text={local.description} />
        </div>

        {local.evidence ? (
          <Property
            name={local.evidence.kind}
            comment={local.evidence.comment}
            value={<code>&quot;{neutralize(local.evidence.target)}&quot;</code>}
          />
        ) : null}

        {local.children ? (
          <div className="local__property">
            <Comment text={local.children.comment} />
            <span className="local__key">children:</span>
            <ul className="local__children">
              {local.children.entries.map((child) => (
                <li key={child.id} className="local__child">
                  <Comment text={child.comment} />
                  <div className="local__child-card">
                    <div className="local__child-id">
                      {reference(child.id)}
                      <ChildStatus child={nodes?.get(child.id)} />
                    </div>
                    {child.description === null ? (
                      <p className="local__missing">(not defined)</p>
                    ) : (
                      <Description text={child.description} />
                    )}
                    <ChildStatusDetail child={nodes?.get(child.id)} />
                  </div>
                </li>
              ))}
            </ul>
          </div>
        ) : null}

        {local.deadline ? (
          <>
            <Property name="until" comment={local.deadline.until_comment} value={neutralize(local.deadline.until)} />
            {local.deadline.promote ? (
              <Property name="promote" comment={local.deadline.promote.comment} value={local.deadline.promote.mode} />
            ) : null}
          </>
        ) : null}
      </section>

      {/* Supplementary: not part of what an approval binds to, and drawn apart from it. */}
      {node.parents.length > 0 || actions ? (
        <aside className="local__context" aria-label="Supplementary navigation">
          {node.parents.length > 0 ? (
            <p className="local__parents">
              <span className="local__context-label">
                {node.parents.length === 1 ? 'Parent' : 'Parents'} (not part of this local view)
              </span>
              {node.parents.map((parent) => (
                <span key={parent} className="local__parent">
                  {reference(parent)}
                </span>
              ))}
            </p>
          ) : null}
          {actions ? <p className="local__actions">{actions}</p> : null}
        </aside>
      ) : null}
    </article>
  )
}

function plainReference(id: string): ReactNode {
  return <span className="reference">{neutralize(id)}</span>
}

/** A child's status beside its id, spelled out. */
function ChildStatus({ child }: { child: NodeData | undefined }) {
  return child ? <StatusBadge status={child.status} /> : null
}

/**
 * What a child's status says beyond its name, at the foot of its card and
 * under a rule: it is where the child stands, not what the child claims.
 */
function ChildStatusDetail({ child }: { child: NodeData | undefined }) {
  const details = child ? childStatusDetail(child) : []
  if (!child || details.length === 0) return null
  return (
    <div className={`local__child-status local__child-status--${child.status.state}`}>
      <span className="visually-hidden">Status of {neutralize(child.id)}: </span>
      {details.map((detail) => (
        <p key={detail}>{neutralize(detail)}</p>
      ))}
    </div>
  )
}

/** One `key: value` property under the comment attached to it. */
function Property({ name, comment, value }: { name: string; comment: string | null; value: ReactNode }) {
  return (
    <div className="local__property">
      <Comment text={comment} />
      <span className="local__key">{name}:</span> <span className="local__value">{value}</span>
    </div>
  )
}

/** A comment, written above what it is attached to as `//` lines. */
function Comment({ text }: { text: string | null }) {
  if (text === null) return null
  return (
    <p className="local__comment">
      <span className="visually-hidden">Comment: </span>
      {neutralize(text)
        .split('\n')
        .map((line, index) => (
          <span key={index} className="local__comment-line">
            {line === '' ? '//' : `// ${line}`}
          </span>
        ))}
    </p>
  )
}
