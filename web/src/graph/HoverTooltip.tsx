/**
 * Hovering a node reveals its full description — the one place the contract
 * text appears in the graph, since it is never drawn inside a node.
 */

import { useEffect, useRef, useState } from 'react'
import type { Core, EventObject } from 'cytoscape'

import { nodeOf, type ArgumentView } from '../model/view'

/** Offset from the cursor, and the margin kept to the window's edges. */
const CURSOR_OFFSET = 16
const EDGE_MARGIN = 8

interface Props {
  readonly cy: Core
  readonly view: ArgumentView
}

export function HoverTooltip({ cy, view }: Props) {
  const [hovered, setHovered] = useState<string | null>(null)
  const tooltip = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const move = (event: EventObject) => {
      const element = tooltip.current
      if (!element) return
      const { x, y } = event.renderedPosition ?? { x: 0, y: 0 }
      const maxX = cy.width() - element.offsetWidth - EDGE_MARGIN
      const maxY = cy.height() - element.offsetHeight - EDGE_MARGIN
      element.style.left = `${Math.max(EDGE_MARGIN, Math.min(x + CURSOR_OFFSET, maxX))}px`
      element.style.top = `${Math.max(EDGE_MARGIN, Math.min(y + CURSOR_OFFSET, maxY))}px`
    }
    const enter = (event: EventObject) => {
      setHovered(event.target.id() as string)
      move(event)
    }
    const leave = () => setHovered(null)

    cy.on('mouseover', 'node', enter)
    cy.on('mouseout', 'node', leave)
    cy.on('mousemove', move)
    return () => {
      cy.off('mouseover', 'node', enter)
      cy.off('mouseout', 'node', leave)
      cy.off('mousemove', move)
    }
  }, [cy])

  if (!hovered || !view.nodes.has(hovered)) return null
  const node = nodeOf(view, hovered)
  return (
    <div className="tooltip" role="tooltip" ref={tooltip}>
      <strong>{node.id}</strong>
      <p>{node.description}</p>
    </div>
  )
}
