/**
 * The controls that record a decision: approve, or decline with a reason.
 *
 * They exist only under a local view shown in full, for a target that can
 * take a decision, on a server that records. The decision names the view hash
 * of the content on screen, and who it is recorded as is the server's — shown
 * here, never chosen here.
 */

import { useId, useState } from 'react'

import type { DecisionData } from '../generated/DecisionData'
import type { ReviewerSession } from '../generated/ReviewerSession'

interface Props {
  readonly reviewer: ReviewerSession
  /** Records the decision; resolves when the server has answered. */
  readonly onDecide: (decision: DecisionData) => Promise<void>
}

export function DecisionBar({ reviewer, onDecide }: Props) {
  const [comment, setComment] = useState('')
  const [busy, setBusy] = useState(false)
  const commentId = useId()
  const hintId = useId()
  const reason = comment.trim()

  const decide = async (decision: DecisionData) => {
    setBusy(true)
    try {
      await onDecide(decision)
    } finally {
      setBusy(false)
    }
  }

  return (
    <form className="decision" aria-label="Decision" onSubmit={(event) => event.preventDefault()}>
      <p className="decision__reviewer">
        Recording as <strong>{reviewer.approver}</strong>
        {reviewer.key_id !== reviewer.approver ? <span> (key {reviewer.key_id})</span> : null}
      </p>
      <label className="decision__label" htmlFor={commentId}>
        Comment
      </label>
      <textarea
        id={commentId}
        className="decision__comment"
        rows={2}
        value={comment}
        aria-describedby={hintId}
        onChange={(event) => setComment(event.target.value)}
      />
      <p id={hintId} className="decision__hint">
        Optional for an approval. A decline must say why.
      </p>
      <div className="decision__actions">
        <button
          type="button"
          className="button button--approve"
          disabled={busy}
          onClick={() => void decide({ label: 'approve', comment: reason === '' ? null : reason })}
        >
          Approve
        </button>
        <button
          type="button"
          className="button button--decline"
          disabled={busy || reason === ''}
          onClick={() => void decide({ label: 'decline', comment: reason })}
        >
          Decline
        </button>
      </div>
    </form>
  )
}
