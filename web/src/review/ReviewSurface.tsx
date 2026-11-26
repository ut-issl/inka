/**
 * The review surface: the review queue, walked one local view at a time.
 *
 * The queue is the one `inka review --list` prints — the same targets, grouped
 * by state and ordered from the leaves up — and the walk is the interactive
 * session's: the targets that can take a decision, in queue order. Both arrive
 * from the server; nothing here decides what is queued or in what order.
 *
 * The order is a recommendation, not a gate: any target can be opened
 * directly. Declined and blocked targets are listed with what resolves them.
 *
 * On a server that records, a target that can take a decision gets the
 * controls to take it, under its local view and nowhere else.
 */

import { useEffect, useRef, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'

import { LocalView } from '../components/LocalView'
import { ReviewBadge } from '../components/StatusBadge'
import { DECISIONS_URL } from '../config'
import type { DecisionData } from '../generated/DecisionData'
import type { NodeData } from '../generated/NodeData'
import type { ReviewerSession } from '../generated/ReviewerSession'
import type { Snapshot } from '../generated/Snapshot'
import { recordDecision } from '../model/session'
import { groupHeading, reviewDetail, takesDecision } from '../model/wording'
import { neutralize } from '../text/neutralize'
import { DecisionBar } from './DecisionBar'

interface Props {
  readonly snapshot: Snapshot
  readonly nodes: ReadonlyMap<string, NodeData>
  /** Who the server records as; `null` on a read-only server. */
  readonly reviewer?: ReviewerSession | null
  /** Asks the server for the page again, after something was decided. */
  readonly onReload?: () => void
}

/** What the last decision came to, when it is worth saying. */
interface Outcome {
  readonly kind: 'recorded' | 'problem'
  readonly message: string
}

export function ReviewSurface({ snapshot, nodes, reviewer = null, onReload }: Props) {
  const params = useParams()
  const navigate = useNavigate()
  const walk = snapshot.queue.walk
  const currentId = params.id ?? walk[0]
  const current = currentId === undefined ? undefined : nodes.get(currentId)
  const [outcome, setOutcome] = useState<Outcome | null>(null)

  // What was said about one target is not said about the next.
  const shownId = current?.id
  useEffect(() => {
    setOutcome((last) => (last?.kind === 'problem' ? null : last))
  }, [shownId])

  const hash = current?.view_hash ?? null
  const decidable = reviewer !== null && current !== undefined && hash !== null && takesDecision(current.review)

  const decide = async (decision: DecisionData) => {
    if (!reviewer || !current || hash === null) return
    const response = await recordDecision(DECISIONS_URL, reviewer, current.id, hash, decision)
    switch (response.outcome) {
      case 'recorded': {
        // The target leaves the walk; move on to the one that followed it.
        const index = walk.indexOf(current.id)
        const next = walk[index + 1] ?? walk.find((id) => id !== current.id)
        setOutcome({ kind: 'recorded', message: `Recorded ${decision.label} of ${current.mode}:${current.id}.` })
        onReload?.()
        void navigate(next === undefined ? '/review' : reviewPath(next))
        return
      }
      case 'changed':
        setOutcome({
          kind: 'problem',
          message:
            'This target has changed since it was shown, so nothing was recorded. Its current content is now on screen.',
        })
        onReload?.()
        return
      case 'refused':
      case 'failed':
        setOutcome({ kind: 'problem', message: `Nothing was recorded: ${response.message}` })
    }
  }

  // A new target is read from its top: the pane does not keep the place the
  // previous one was scrolled to.
  const main = useRef<HTMLElement>(null)
  const shownTarget = current?.id
  useEffect(() => {
    if (main.current) main.current.scrollTop = 0
  }, [shownTarget])

  return (
    <div className="review">
      <Queue snapshot={snapshot} nodes={nodes} currentId={current?.id} />
      <section ref={main} className="review__main" aria-label="Review target">
        {outcome ? (
          <p className={`outcome outcome--${outcome.kind}`} role={outcome.kind === 'problem' ? 'alert' : 'status'}>
            {neutralize(outcome.message)}
          </p>
        ) : null}
        {current ? (
          <>
            <Progress walk={walk} currentId={current.id} />
            <LocalView node={current} reference={(id) => <TargetLink id={id} />} />
            {decidable ? <DecisionBar key={`${current.id}@${hash}`} reviewer={reviewer} onDecide={decide} /> : null}
          </>
        ) : (
          <Empty requested={params.id} queued={walk.length} />
        )}
      </section>
    </div>
  )
}

/** Where to find a target on this surface. */
export function reviewPath(id: string): string {
  return `/review/${encodeURIComponent(id)}`
}

function TargetLink({ id }: { id: string }) {
  return (
    <Link className="reference" to={reviewPath(id)}>
      {neutralize(id)}
    </Link>
  )
}

/** The queue as `inka review --list` prints it: one section per state. */
function Queue({
  snapshot,
  nodes,
  currentId,
}: Pick<Props, 'snapshot' | 'nodes'> & { currentId: string | undefined }) {
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
                  <Link
                    to={reviewPath(id)}
                    className={`queue__target ${id === currentId ? 'is-current' : ''}`}
                    aria-current={id === currentId ? 'true' : undefined}
                  >
                    <span className="queue__mode">{node.mode}:</span>
                    <span className="queue__id">{neutralize(id)}</span>
                    {detail ? <span className="queue__detail">{neutralize(detail)}</span> : null}
                  </Link>
                </li>
              )
            })}
          </ul>
        </section>
      ))}
    </nav>
  )
}

/** Where the target sits in the walk, what remains, and the way on. */
function Progress({ walk, currentId }: { walk: readonly string[]; currentId: string }) {
  const index = walk.indexOf(currentId)
  const previous = index > 0 ? walk[index - 1] : undefined
  const next = index >= 0 ? walk[index + 1] : walk[0]
  return (
    <div className="progress">
      <p className="progress__position" aria-live="polite">
        {index >= 0
          ? `Target ${index + 1} of ${walk.length} · ${walk.length - index - 1} after this one`
          : `Not in the walk · ${walk.length} target${walk.length === 1 ? '' : 's'} can take a decision`}
      </p>
      <div className="progress__moves">
        {previous !== undefined ? (
          <Link className="button" to={reviewPath(previous)}>
            Previous
          </Link>
        ) : null}
        {next !== undefined ? (
          <Link className="button" to={reviewPath(next)}>
            {index >= 0 ? 'Skip' : 'Start the walk'}
          </Link>
        ) : null}
      </div>
    </div>
  )
}

function Empty({ requested, queued }: { requested: string | undefined; queued: number }) {
  if (requested !== undefined) {
    return (
      <p className="notice" role="alert">
        The argument has no block named <code>{neutralize(requested)}</code>.
      </p>
    )
  }
  return <p className="notice">{queued === 0 ? 'Nothing needs a decision.' : null}</p>
}

/** The review state of a node as a badge, for surfaces that list targets. */
export function QueueBadge({ node }: { node: NodeData }) {
  return node.review ? <ReviewBadge state={node.review} /> : null
}
