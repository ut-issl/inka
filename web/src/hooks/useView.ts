/** Loads the page once, and reports how that went. */

import { useEffect, useState } from 'react'

import type { Finding } from '../generated/Finding'
import { loadPage } from '../model/source'
import { viewOf, type ArgumentView } from '../model/view'

export type ViewState =
  | { readonly status: 'loading' }
  | { readonly status: 'ready'; readonly view: ArgumentView }
  /** The project does not check; these are the findings `inka check` reports. */
  | { readonly status: 'rejected'; readonly findings: readonly Finding[] }
  /** The server could not be asked at all. */
  | { readonly status: 'error'; readonly message: string }

export function useView(url: string): ViewState {
  const [state, setState] = useState<ViewState>({ status: 'loading' })

  useEffect(() => {
    const controller = new AbortController()
    let live = true
    setState({ status: 'loading' })

    loadPage(url, controller.signal)
      .then((page) => {
        if (!live) return
        if (page.kind === 'rejected') setState({ status: 'rejected', findings: page.findings })
        else setState({ status: 'ready', view: viewOf(page) })
      })
      .catch((error: unknown) => {
        if (!live || controller.signal.aborted) return
        setState({ status: 'error', message: error instanceof Error ? error.message : String(error) })
      })

    return () => {
      live = false
      controller.abort()
    }
  }, [url])

  return state
}
