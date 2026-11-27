/**
 * The recording session, and the one write the Web UI can make.
 *
 * Whether decisions can be recorded, and as whom, is the server's to say: it
 * was fixed when `inka serve` started, and the page can neither choose nor
 * change it. A decision is one `POST` that carries the session's token and
 * names the view hash that was on screen; the server records it only if that
 * is still the target's current hash.
 */

import type { DecisionData } from '../generated/DecisionData'
import type { DecisionRequest } from '../generated/DecisionRequest'
import type { DecisionResponse } from '../generated/DecisionResponse'
import type { ReviewerSession } from '../generated/ReviewerSession'
import type { Session } from '../generated/Session'

/**
 * Asks the server who it records as. A server that cannot be asked is treated
 * as one that does not record: the page then offers no decision.
 */
export async function loadSession(url: string, signal?: AbortSignal): Promise<Session> {
  try {
    const response = await fetch(url, { signal: signal ?? null, headers: { Accept: 'application/json' } })
    if (!response.ok) return { reviewer: null }
    const session = (await response.json()) as Partial<Session>
    return { reviewer: session.reviewer ?? null }
  } catch {
    return { reviewer: null }
  }
}

/**
 * Asks the server to record one decision on `target`, taken on the content
 * that hashed to `viewHash`. Never throws: a request that could not be made
 * or was turned away is a `failed` outcome with the reason.
 */
export async function recordDecision(
  url: string,
  reviewer: ReviewerSession,
  target: string,
  viewHash: string,
  decision: DecisionData,
): Promise<DecisionResponse> {
  const request: DecisionRequest = { target, view_hash: viewHash, decision }
  let response: Response
  try {
    response = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Inka-Session': reviewer.token },
      body: JSON.stringify(request),
    })
  } catch (error) {
    return { outcome: 'failed', message: `could not reach the server: ${text(error)}` }
  }
  try {
    return (await response.clone().json()) as DecisionResponse
  } catch {
    const body = await response.text().catch(() => '')
    return { outcome: 'failed', message: body.trim() || `the server answered ${response.status}` }
  }
}

function text(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
