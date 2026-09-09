/**
 * The viewer shell: node list, graph, detail — three surfaces over one shared
 * view, holding one piece of state between them (the current focus).
 *
 * The whole application is read-only. Nothing here writes to the argument, to
 * approvals, or to any other inka state; the only outbound request is the one
 * that fetched the argument.
 */

import { useCallback, useEffect, useMemo, useState } from 'react'

import { DetailPanel } from './components/DetailPanel'
import { Legend } from './components/Legend'
import { NodeList } from './components/NodeList'
import { ELEMENTS_URL } from './config'
import { GraphCanvas } from './graph/GraphCanvas'
import { namespaceColors } from './graph/palette'
import { useView } from './hooks/useView'
import { resolveFocus, WHOLE_VIEW, type Focus } from './model/focus'
import { namespacesOf, type ArgumentView } from './model/view'

export function App() {
  const state = useView(ELEMENTS_URL)

  if (state.status === 'loading') {
    return <p className="notice">Loading the argument…</p>
  }
  if (state.status === 'error') {
    return (
      <div className="notice notice--error" role="alert">
        <p>Could not load the argument.</p>
        <pre>{state.message}</pre>
      </div>
    )
  }
  return <Viewer view={state.view} />
}

function Viewer({ view }: { view: ArgumentView }) {
  const [focus, setFocus] = useState<Focus>(WHOLE_VIEW)

  const sets = useMemo(() => resolveFocus(view, focus), [view, focus])
  const colors = useMemo(() => namespaceColors(namespacesOf(view)), [view])

  const focusNode = useCallback((id: string) => setFocus({ kind: 'node', id }), [])
  const focusNamespace = useCallback((name: string) => setFocus({ kind: 'namespace', name }), [])
  const clearFocus = useCallback(() => setFocus(WHOLE_VIEW), [])

  // Esc clears the focus, as a tap on the empty background does.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') clearFocus()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [clearFocus])

  return (
    <main className="viewer">
      <aside className="viewer__list">
        <NodeList
          view={view}
          focus={focus}
          sets={sets}
          colors={colors}
          onFocusNode={focusNode}
          onFocusNamespace={focusNamespace}
        />
      </aside>

      <GraphCanvas
        view={view}
        focus={focus}
        sets={sets}
        colors={colors}
        onFocusNode={focusNode}
        onFocusNamespace={focusNamespace}
        onClearFocus={clearFocus}
      />

      <aside className="viewer__side">
        <Legend />
        <DetailPanel view={view} focus={focus} sets={sets} />
      </aside>
    </main>
  )
}
