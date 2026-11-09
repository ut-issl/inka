/**
 * Hovering a node reveals its full description — the one place the contract
 * text appears in the graph, since it is never drawn inside a node. Hovering
 * an edge whose `children:` entry carries a comment reveals that comment.
 */

import { useEffect, useRef, useState } from 'react'
import type { Core, EventObject } from 'cytoscape'

import type { ArgumentView } from '../model/view'
import { tooltipContent, type Hovered } from './tooltipContent'

/** Offset from the cursor, and the margin kept to the window's edges. */
const CURSOR_OFFSET = 16
const EDGE_MARGIN = 8

interface Props {
  readonly cy: Core
  readonly view: ArgumentView
}

export function HoverTooltip({ cy, view }: Props) {
  const [hovered, setHovered] = useState<Hovered | null>(null)
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
    const enter = (kind: Hovered['kind']) => (event: EventObject) => {
      setHovered({ kind, id: event.target.id() as string })
      move(event)
    }
    const enterNode = enter('node')
    const enterEdge = enter('edge')
    const leave = () => setHovered(null)

    cy.on('mouseover', 'node', enterNode)
    cy.on('mouseover', 'edge', enterEdge)
    cy.on('mouseout', 'node, edge', leave)
    cy.on('mousemove', move)
    return () => {
      // A new graph starts with nothing hovered.
      setHovered(null)
      cy.off('mouseover', 'node', enterNode)
      cy.off('mouseover', 'edge', enterEdge)
      cy.off('mouseout', 'node, edge', leave)
      cy.off('mousemove', move)
    }
  }, [cy])

  const content = hovered ? tooltipContent(view, hovered) : null
  if (!content) return null
  return (
    <div className="tooltip" role="tooltip" ref={tooltip}>
      <strong>{content.title}</strong>
      <p>{content.text}</p>
    </div>
  )
}
