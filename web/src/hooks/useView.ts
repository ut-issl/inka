/** Loads the shared argument view once, and reports how that went. */

import { useEffect, useState } from 'react'

import { loadView } from '../model/source'
import type { ArgumentView } from '../model/view'

export type ViewState =
  | { readonly status: 'loading' }
  | { readonly status: 'ready'; readonly view: ArgumentView }
  | { readonly status: 'error'; readonly message: string }

export function useView(url: string): ViewState {
  const [state, setState] = useState<ViewState>({ status: 'loading' })

  useEffect(() => {
    const controller = new AbortController()
    let live = true
    setState({ status: 'loading' })

    loadView(url, controller.signal)
      .then((view) => {
        if (live) setState({ status: 'ready', view })
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
