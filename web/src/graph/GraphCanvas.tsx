/**
 * The graph pane.
 *
 * It owns the cytoscape instance and translates focus into three things: which
 * elements are visible, which layout runs, and which node is emphasized. The
 * argument itself is never touched — this component only ever reads the view.
 */

import { useEffect, useRef, useState } from 'react'
import cytoscape, { type Core } from 'cytoscape'
import dagre from 'cytoscape-dagre'

import type { Focus, FocusSets } from '../model/focus'
import { namespacesOf, type ArgumentView } from '../model/view'
import { cytoscapeElements } from './elements'
import { HoverTooltip } from './HoverTooltip'
import { NamespaceBadges } from './NamespaceBadges'
import type { NamespaceColors } from './palette'
import { applyLayout, reflow, type LayoutPlan } from './runLayout'
import { stylesheet } from './stylesheet'
import type { Positions } from './layout'

cytoscape.use(dagre)

interface Props {
  readonly view: ArgumentView
  readonly focus: Focus
  readonly sets: FocusSets
  readonly colors: NamespaceColors
  readonly onFocusNode: (id: string) => void
  readonly onFocusNamespace: (name: string) => void
  readonly onClearFocus: () => void
}

export function GraphCanvas({
  view,
  focus,
  sets,
  colors,
  onFocusNode,
  onFocusNamespace,
  onClearFocus,
}: Props) {
  const container = useRef<HTMLDivElement>(null)
  const [cy, setCy] = useState<Core | null>(null)
  const base = useRef<Positions>(new Map())

  // Handlers live in a ref so re-rendering never re-registers graph events.
  const handlers = useRef({ onFocusNode, onClearFocus })
  handlers.current = { onFocusNode, onClearFocus }

  useEffect(() => {
    if (!container.current) return
    const instance = cytoscape({
      container: container.current,
      elements: cytoscapeElements(view),
      style: stylesheet(),
      // Read-only: the graph answers taps, it does not let anything be moved.
      autoungrabify: true,
      boxSelectionEnabled: false,
    })

    instance.on('tap', 'node', (event) => handlers.current.onFocusNode(event.target.id() as string))
    instance.on('tap', (event) => {
      if (event.target === instance) handlers.current.onClearFocus()
    })

    setCy(instance)
    return () => {
      instance.destroy()
      setCy(null)
    }
  }, [view])

  // Focus drives visibility, emphasis and which layout runs.
  useEffect(() => {
    if (!cy) return
    let live = true

    cy.batch(() => {
      const { visible } = sets
      // An edge is shown only when both of its endpoints are.
      const shown =
        visible === null
          ? cy.elements()
          : cy.elements().filter((element) =>
              element.isNode()
                ? visible.has(element.id())
                : visible.has(element.source().id()) && visible.has(element.target().id()),
            )
      // `display` is what cytoscape's `:visible` reads, and what the layout
      // passes and the badges then follow.
      cy.elements().not(shown).style('display', 'none')
      shown.style('display', 'element')

      // Emphasis marks one focused node. A focused namespace is already almost
      // everything on screen, so rimming its members would only cost the mode
      // colours their legibility.
      cy.nodes().removeClass('focused')
      if (focus.kind === 'node') {
        cy.nodes().filter((node) => sets.primary.has(node.id())).addClass('focused')
      }
    })

    const plan: LayoutPlan =
      focus.kind === 'node'
        ? { kind: 'ego', focusId: focus.id, namespaces: namespacesOf(view) }
        : { kind: 'layered' }

    void applyLayout(cy, plan).then((positions) => {
      // A newer focus may have landed while dagre was running; that layout owns
      // the base from here on.
      if (live) base.current = positions
    })

    return () => {
      live = false
    }
  }, [cy, view, focus, sets])

  // The laid-out graph matches the window's aspect ratio, so a resize re-runs
  // the stretch from the same base layout.
  useEffect(() => {
    if (!cy || !container.current) return
    const observer = new ResizeObserver(() => {
      cy.resize()
      if (base.current.size > 0) reflow(cy, base.current, false)
    })
    observer.observe(container.current)
    return () => observer.disconnect()
  }, [cy])

  return (
    // The cytoscape canvases own their container; the overlays are siblings so
    // React and cytoscape never manage the same children.
    <div className="graph">
      <div className="graph__canvas" ref={container} />
      {cy ? (
        <>
          <NamespaceBadges cy={cy} view={view} colors={colors} onFocusNamespace={onFocusNamespace} />
          <HoverTooltip cy={cy} view={view} />
        </>
      ) : null}
    </div>
  )
}
