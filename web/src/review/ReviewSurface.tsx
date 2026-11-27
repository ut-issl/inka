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
import { NodeReference } from '../components/NodeReference'
import { Sidebar } from '../components/Sidebar'
import { DECISIONS_URL } from '../config'
import type { DecisionData } from '../generated/DecisionData'
import type { NodeData } from '../generated/NodeData'
import type { ReviewerSession } from '../generated/ReviewerSession'
import type { Snapshot } from '../generated/Snapshot'
import { recordDecision } from '../model/session'
import { takesDecision } from '../model/wording'
import { graphPath, reviewPath } from '../paths'
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
  /**
   * The decisions this page has had recorded, and the page they were taken
   * on. Until the server's next answer replaces that page, their targets are
   * still on screen as if undecided; they take no second decision meanwhile.
   */
  const [recorded, setRecorded] = useState<{ on: Snapshot; keys: ReadonlySet<string> } | null>(null)

  // What was said about one target is not said about the next.
  const shownId = current?.id
  /** The target on screen now, for a decision that completes later to compare with. */
  const shownRef = useRef(shownId)
  useEffect(() => {
    shownRef.current = shownId
    setOutcome((last) => (last?.kind === 'problem' ? null : last))
  }, [shownId])

  const hash = current?.view_hash ?? null
  const key = current === undefined || hash === null ? null : `${current.id}@${hash}`
  const alreadyRecorded = recorded !== null && recorded.on === snapshot && key !== null && recorded.keys.has(key)
  const decidable =
    reviewer !== null &&
    current !== undefined &&
    hash !== null &&
    takesDecision(current.review) &&
    !alreadyRecorded

  const decide = async (decision: DecisionData) => {
    if (!reviewer || !current || hash === null || key === null) return
    const target = `${current.mode}:${current.id}`
    const response = await recordDecision(DECISIONS_URL, reviewer, current.id, hash, decision)
    // The reviewer may have opened another target while the server answered.
    const stillShown = shownRef.current === current.id
    switch (response.outcome) {
      case 'recorded': {
        setRecorded((last) => ({
          on: snapshot,
          keys: new Set([...(last?.on === snapshot ? last.keys : []), key]),
        }))
        setOutcome({ kind: 'recorded', message: `Recorded ${decision.label} of ${target}.` })
        onReload?.()
        // The target leaves the walk; move on to the one that followed it —
        // unless the reviewer has already gone somewhere of their own choosing.
        if (stillShown) {
          const index = walk.indexOf(current.id)
          const next = walk[index + 1] ?? walk.find((id) => id !== current.id)
          void navigate(next === undefined ? '/review' : reviewPath(next))
        }
        return
      }
      case 'changed':
        setOutcome({
          kind: 'problem',
          message: stillShown
            ? `${target} has changed since it was shown, so nothing was recorded. Its current content is now on screen.`
            : `${target} has changed since it was shown, so nothing was recorded.`,
        })
        onReload?.()
        return
      case 'refused':
      case 'failed':
        setOutcome({ kind: 'problem', message: `Nothing was recorded for ${target}: ${response.message}` })
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
      <Sidebar snapshot={snapshot} nodes={nodes} surface="review" currentId={current?.id} />
      <section ref={main} className="review__main" aria-label="Review target">
        {outcome ? (
          <p className={`outcome outcome--${outcome.kind}`} role={outcome.kind === 'problem' ? 'alert' : 'status'}>
            {neutralize(outcome.message)}
          </p>
        ) : null}
        {current ? (
          <>
            <Progress walk={walk} currentId={current.id} />
            <LocalView
              node={current}
              reference={(id) => <NodeReference node={nodes.get(id)} id={id} to={reviewPath(id)} />}
              actions={
                <Link className="button" to={graphPath(current.id)}>
                  Show in graph
                </Link>
              }
            />
            {decidable ? <DecisionBar key={`${current.id}@${hash}`} reviewer={reviewer} onDecide={decide} /> : null}
          </>
        ) : (
          <Empty requested={params.id} queued={walk.length} />
        )}
      </section>
    </div>
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
