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
import { useCallback, useEffect, useMemo, useRef } from 'react'

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
  /** The node whose local view is open beside the graph, if any. */
  readonly selectedId: string | undefined
  readonly onToggle: (id: string) => void
  readonly onSelect: (id: string) => void
  /** A tap on the empty background. */
  readonly onClear: () => void
  /** Changes whenever the room the canvas has changes, so the view is framed again. */
  readonly frame?: unknown
}

export function CardGraph(props: Props) {
  return (
    <ReactFlowProvider>
      <Canvas {...props} />
    </ReactFlowProvider>
  )
}

function Canvas({ nodes, visible, expanded, unreachable, selectedId, onToggle, onSelect, onClear, frame }: Props) {
  const flow = useReactFlow<CardNode>()
  const [cards, setCards, onCardsChange] = useNodesState<CardNode>([])
  const measured = useNodesInitialized()
  /** The set of cards the positions on screen were computed for. */
  const laidOut = useRef('')
  const selectedRef = useRef(selectedId)
  selectedRef.current = selectedId

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
            data: {
              node,
              expanded: expanded.has(id),
              unreachable: unreachable.has(id),
              selected: id === selectedId,
              onToggle,
              onSelect,
            },
          },
        ]
      })
    })
  }, [nodes, visible, expanded, unreachable, selectedId, onToggle, onSelect, setCards])

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

  /**
   * Frames the view: the whole graph, or, with a node selected, that node
   * and what surrounds it.
   */
  const reframe = useCallback(() => {
    const focus = selectedRef.current
    if (focus !== undefined && flow.getNode(focus)) {
      void flow.fitView({ nodes: [{ id: focus }], padding: 3.5, duration: 200, maxZoom: 1 })
    } else {
      void flow.fitView({ padding: 0.1, duration: 200, maxZoom: 1 })
    }
  }, [flow])

  // A selection made elsewhere — the sidebar, a link in a local view — may be
  // of a card that is already placed but out of sight, and the room the
  // canvas has may change under it. Neither moves a card, so no layout runs;
  // the view is framed again all the same. A card still waiting for its
  // layout is framed when that layout lands.
  useEffect(() => {
    if (selectedId === undefined) return
    const card = flow.getNode(selectedId)
    if (card && card.style?.opacity !== 0) reframe()
  }, [selectedId, frame, flow, reframe])

  // Once every card on the canvas has been measured, place them.
  useEffect(() => {
    if (!measured || cards.length === 0) return
    // Everything the layout is a function of: the cards, their sizes, and the
    // edges between them. An edge can appear between two cards that were both
    // already shown, and it constrains where they may go.
    const key = [
      ...cards.map((card) => `${card.id}:${card.measured?.width}x${card.measured?.height}`),
      ...edges.map((edge) => edge.id),
    ].join('|')
    // A card that has not been placed yet is waiting, unseen, for a layout —
    // even when the set of cards has come back to one already laid out.
    const unplaced = cards.some((card) => card.style?.opacity === 0)
    if (key === laidOut.current && !unplaced) return
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
      window.setTimeout(reframe, 50)
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
      onNodeClick={(_event, card) => onSelect(card.id)}
      onPaneClick={onClear}
      nodesDraggable={false}
      nodesFocusable={false}
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
