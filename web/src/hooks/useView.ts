/** Loads the page, and reports how that went. */

import { useCallback, useEffect, useState } from 'react'

import type { Finding } from '../generated/Finding'
import type { Snapshot } from '../generated/Snapshot'
import { loadPage } from '../model/source'
import { viewOf, type ArgumentView } from '../model/view'

export type ViewState =
  | { readonly status: 'loading' }
  | {
      readonly status: 'ready'
      /** The page as the server sent it. */
      readonly snapshot: Snapshot
      /** The same page, indexed for the surfaces that draw it. */
      readonly view: ArgumentView
    }
  /** The project does not check; these are the findings `inka check` reports. */
  | { readonly status: 'rejected'; readonly findings: readonly Finding[] }
  /** The server could not be asked at all. */
  | { readonly status: 'error'; readonly message: string }

export interface PageHandle {
  readonly state: ViewState
  /**
   * Asks the server again. The page on screen stays until the new one
   * arrives, so a reload does not flash through the loading state.
   */
  readonly reload: () => void
}

export function usePage(url: string): PageHandle {
  const [state, setState] = useState<ViewState>({ status: 'loading' })
  const [request, setRequest] = useState(0)

  useEffect(() => {
    const controller = new AbortController()
    let live = true

    loadPage(url, controller.signal)
      .then((page) => {
        if (!live) return
        if (page.kind === 'rejected') setState({ status: 'rejected', findings: page.findings })
        else setState({ status: 'ready', snapshot: page, view: viewOf(page) })
      })
      .catch((error: unknown) => {
        if (!live || controller.signal.aborted) return
        setState({ status: 'error', message: error instanceof Error ? error.message : String(error) })
      })

    return () => {
      live = false
      controller.abort()
    }
  }, [url, request])

  const reload = useCallback(() => setRequest((count) => count + 1), [])
  return { state, reload }
}

/** The page's state alone, for callers that never reload. */
export function useView(url: string): ViewState {
  return usePage(url).state
}
