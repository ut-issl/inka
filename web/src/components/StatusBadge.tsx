/** A node's status and review state, spelled out; colour only repeats them. */

import type { ReviewState } from '../generated/ReviewState'
import type { Status } from '../generated/Status'
import { reviewLabel, statusLabel } from '../model/wording'

export function StatusBadge({ status }: { status: Status }) {
  return <span className={`pill pill--status-${status.state}`}>{statusLabel(status)}</span>
}

export function ReviewBadge({ state }: { state: ReviewState }) {
  return <span className={`pill pill--review-${state.state}`}>{reviewLabel(state)}</span>
}
