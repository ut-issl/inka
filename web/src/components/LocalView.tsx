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
 */

import type { ReactNode } from 'react'

import type { NodeData } from '../generated/NodeData'
import { reviewDetail, statusDetail } from '../model/wording'
import { neutralize } from '../text/neutralize'
import { Description } from './Description'
import { ReviewBadge, StatusBadge } from './StatusBadge'

interface Props {
  readonly node: NodeData
  /** Draws a reference to another node; a plain id unless the surface links it. */
  readonly reference?: (id: string) => ReactNode
}

export function LocalView({ node, reference = plainReference }: Props) {
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
                    <div className="local__child-id">{reference(child.id)}</div>
                    {child.description === null ? (
                      <p className="local__missing">(not defined)</p>
                    ) : (
                      <Description text={child.description} />
                    )}
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
    </article>
  )
}

function plainReference(id: string): ReactNode {
  return <span className="reference">{neutralize(id)}</span>
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
