/**
 * How a node's status and review state are spelled out.
 *
 * What the status *is* arrives from the server; this only words it, the way
 * `inka eval` and `inka review` word the same things, so a reader who knows
 * one recognises the other. Every state is named in text — colour repeats it
 * and never carries it alone.
 */

import type { BlameReason } from '../generated/BlameReason'
import type { BlockedReason } from '../generated/BlockedReason'
import type { ReviewGroup } from '../generated/ReviewGroup'
import type { ReviewState } from '../generated/ReviewState'
import type { Status } from '../generated/Status'

/** The status in a word or two, for a badge. */
export function statusLabel(status: Status): string {
  switch (status.state) {
    case 'established':
      return 'established'
    case 'blamed':
      return 'blamed'
    case 'unestablished':
      return 'not established'
  }
}

/** Why the status is what it is; nothing to add for an established node. */
export function statusDetail(status: Status): string[] {
  switch (status.state) {
    case 'established':
      return []
    case 'blamed':
      return status.reasons.map(blameReason)
    case 'unestablished':
      return ['something below it is not established']
  }
}

/** One cause a node fails on its own, as `inka eval` reports it. */
export function blameReason(reason: BlameReason): string {
  switch (reason.cause) {
    case 'missing_approval':
      return 'no approval covers the current content'
    case 'stale_approval':
      return `approvals are stale: ${reason.approvers.join(', ')} approved an earlier version`
    case 'declined':
      return `declined by ${reason.by}: ${reason.comment}`
    case 'until_expired':
      return `approvals no longer count: the current phase is past until:${reason.until}`
    case 'missing_evidence':
      return `no imported test evidence provides \`${reason.target}\``
    case 'evidence_failed':
      return `required executions failed: ${reason.cases.join(', ')}`
    case 'evidence_incomplete':
      return `required executions were skipped, cancelled or missing: ${reason.cases.join(', ')}`
    case 'veriq_not_evaluated':
      return 'veriq evidence is not evaluated yet'
  }
}

/** A review state as a target's header spells it. */
export function reviewLabel(state: ReviewState): string {
  return state.state
}

/** A queue section's heading. */
export function groupHeading(group: ReviewGroup): string {
  switch (group) {
    case 'unapproved':
      return 'Unapproved'
    case 'stale':
      return 'Stale'
    case 'declined':
      return 'Declined'
    case 'blocked':
      return 'Blocked'
  }
}

/** What a review state says beyond its name, as `inka review` words it. */
export function reviewDetail(state: ReviewState): string | null {
  switch (state.state) {
    case 'unapproved':
      return null
    case 'stale':
      return `${state.approvers.join(', ')} approved an earlier version`
    case 'declined':
      return `by ${state.by.join(', ')} (edit or retract to resolve)`
    case 'blocked':
      return blockedReason(state.reason)
  }
}

function blockedReason(reason: BlockedReason): string {
  switch (reason.cause) {
    case 'missing_evidence':
      return `no imported test evidence provides \`${reason.target}\``
    case 'veriq_not_evaluated':
      return 'veriq evidence is not evaluated yet'
    case 'until_expired':
      return `approvals no longer count: the current phase is past until:${reason.until}`
  }
}

/** Whether a decision can be taken on a node in this review state. */
export function takesDecision(state: ReviewState | null): boolean {
  return state !== null && (state.state === 'unapproved' || state.state === 'stale')
}
