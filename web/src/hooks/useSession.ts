/** Asks once who the server records as; `null` until it has answered, and when it records as nobody. */

import { useEffect, useState } from 'react'

import type { ReviewerSession } from '../generated/ReviewerSession'
import { loadSession } from '../model/session'

export function useSession(url: string): ReviewerSession | null {
  const [reviewer, setReviewer] = useState<ReviewerSession | null>(null)

  useEffect(() => {
    const controller = new AbortController()
    let live = true
    void loadSession(url, controller.signal).then((session) => {
      if (live) setReviewer(session.reviewer)
    })
    return () => {
      live = false
      controller.abort()
    }
  }, [url])

  return reviewer
}
