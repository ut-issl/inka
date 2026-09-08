/**
 * The namespace badges: one coloured chip per node, sitting at its top-right
 * corner in every state, with no enclosing box or outline anywhere.
 *
 * A badge is a focus target for its namespace, and it disappears below a zoom
 * threshold so a zoomed-out graph stays readable.
 *
 * Positions follow the graph's own render loop and are written straight to the
 * DOM: re-rendering React on every pan frame would be wasted work.
 */

import { useEffect, useRef } from 'react'
import type { Core } from 'cytoscape'

import { BADGE_MIN_ZOOM } from '../config'
import { colorOf, type NamespaceColors } from './palette'
import { nodeOf, type ArgumentView } from '../model/view'

interface Props {
  readonly cy: Core
  readonly view: ArgumentView
  readonly colors: NamespaceColors
  readonly onFocusNamespace: (name: string) => void
}

export function NamespaceBadges({ cy, view, colors, onFocusNamespace }: Props) {
  const badges = useRef(new Map<string, HTMLButtonElement>())

  useEffect(() => {
    const place = () => {
      const zoomedIn = cy.zoom() >= BADGE_MIN_ZOOM
      for (const [id, element] of badges.current) {
        const node = cy.getElementById(id)
        if (!zoomedIn || node.empty() || !node.visible()) {
          element.style.display = 'none'
          continue
        }
        const box = node.renderedBoundingBox({ includeLabels: false })
        element.style.display = 'block'
        element.style.left = `${box.x2}px`
        element.style.top = `${box.y1}px`
      }
    }

    cy.on('render', place)
    place()
    return () => {
      cy.off('render', place)
    }
  }, [cy])

  return (
    <div className="badges">
      {view.order.map((id) => {
        const namespace = nodeOf(view, id).namespace
        return (
          <button
            key={id}
            type="button"
            className="badge"
            style={{ backgroundColor: colorOf(colors, namespace) }}
            title={`Focus namespace ${namespace}`}
            ref={(element) => {
              if (element) badges.current.set(id, element)
              else badges.current.delete(id)
            }}
            onClick={() => onFocusNamespace(namespace)}
          >
            {namespace}
          </button>
        )
      })}
    </div>
  )
}
