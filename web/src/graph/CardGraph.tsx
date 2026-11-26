/**
 * The graph surface's canvas: the visible cards, laid out top-down, with an
 * edge from each parent to each of its visible children.
 *
 * The canvas itself — pan, zoom, edges with arrowheads, cards as DOM elements
 * — is React Flow's. What is on it is decided in two pure steps this component
 * only connects: `visibility` says which nodes are shown, and `layout` says
 * where, from the sizes the cards turn out to have once drawn.
 */

import {
  Background,
  Controls,
  MarkerType,
  ReactFlow,
  ReactFlowProvider,
  useNodesInitialized,
  useNodesState,
  useReactFlow,
  type Edge,
} from '@xyflow/react'
import { useEffect, useMemo, useRef } from 'react'

import '@xyflow/react/dist/style.css'

import type { NodeData } from '../generated/NodeData'
import { Card, type CardNode } from './Card'
import { layoutCards } from './layout'

const nodeTypes = { card: Card }

interface Props {
  readonly nodes: ReadonlyMap<string, NodeData>
  /** The visible node ids, parents before children. */
  readonly visible: readonly string[]
  readonly expanded: ReadonlySet<string>
  readonly unreachable: ReadonlySet<string>
  readonly onToggle: (id: string) => void
}

export function CardGraph(props: Props) {
  return (
    <ReactFlowProvider>
      <Canvas {...props} />
    </ReactFlowProvider>
  )
}

function Canvas({ nodes, visible, expanded, unreachable, onToggle }: Props) {
  const flow = useReactFlow<CardNode>()
  const [cards, setCards, onCardsChange] = useNodesState<CardNode>([])
  const measured = useNodesInitialized()
  /** The set of cards the positions on screen were computed for. */
  const laidOut = useRef('')

  // The cards on the canvas follow the visible set. A card keeps the position
  // it has; a new one waits, unseen, until the layout has placed it.
  useEffect(() => {
    setCards((current) => {
      const placed = new Map(current.map((card) => [card.id, card]))
      return visible.flatMap((id) => {
        const node = nodes.get(id)
        if (!node) return []
        const before = placed.get(id)
        return [
          {
            id,
            type: 'card' as const,
            position: before?.position ?? { x: 0, y: 0 },
            draggable: false,
            connectable: false,
            style: { opacity: before ? 1 : 0 },
            data: { node, expanded: expanded.has(id), unreachable: unreachable.has(id), onToggle },
          },
        ]
      })
    })
  }, [nodes, visible, expanded, unreachable, onToggle, setCards])

  const edges = useMemo<Edge[]>(() => {
    const shown = new Set(visible)
    return visible.flatMap((id) => {
      const node = nodes.get(id)
      if (!node || !expanded.has(id)) return []
      return (node.local.children?.entries ?? [])
        .filter((entry) => shown.has(entry.id))
        .map((entry) => ({
          id: `${id}->${entry.id}`,
          source: id,
          target: entry.id,
          type: 'smoothstep',
          selectable: false,
          focusable: false,
          markerEnd: { type: MarkerType.ArrowClosed },
          // An entry that carries a comment says so on its edge; the comment
          // itself is read in the parent's local view.
          ...(entry.comment === null
            ? {}
            : { label: '//', ariaLabel: `${id} to ${entry.id}: the entry carries a comment`, className: 'edge--commented' }),
        }))
    })
  }, [nodes, visible, expanded])

  // Once every card on the canvas has been measured, place them.
  useEffect(() => {
    if (!measured || cards.length === 0) return
    const key = cards.map((card) => `${card.id}:${card.measured?.width}x${card.measured?.height}`).join('|')
    if (key === laidOut.current) return
    let live = true
    void layoutCards(
      cards.map((card) => ({ id: card.id, width: card.measured?.width ?? 0, height: card.measured?.height ?? 0 })),
      edges,
    ).then((positions) => {
      if (!live) return
      laidOut.current = key
      setCards((current) =>
        current.map((card) => ({
          ...card,
          position: positions.get(card.id) ?? card.position,
          style: { opacity: 1 },
        })),
      )
      // Once the new positions are on the canvas.
      window.setTimeout(() => void flow.fitView({ padding: 0.1, duration: 200, maxZoom: 1 }), 50)
    })
    return () => {
      live = false
    }
  }, [measured, cards, edges, setCards, flow])

  return (
    <ReactFlow
      className="graph"
      nodes={cards}
      edges={edges}
      nodeTypes={nodeTypes}
      onNodesChange={onCardsChange}
      nodesDraggable={false}
      nodesConnectable={false}
      edgesFocusable={false}
      minZoom={0.1}
      maxZoom={1.5}
      proOptions={{ hideAttribution: false }}
      aria-label="Argument graph"
    >
      <Background />
      <Controls showInteractive={false} />
    </ReactFlow>
  )
}
