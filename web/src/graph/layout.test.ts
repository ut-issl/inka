import { describe, expect, test } from 'vitest'

import { layoutCards, type CardEdge, type CardSize, type Point } from './layout'

/** Cards of one width and differing heights, as text makes them. */
const cards: CardSize[] = [
  { id: 'root', width: 300, height: 120 },
  { id: 'left', width: 300, height: 90 },
  { id: 'right', width: 300, height: 210 },
  { id: 'shared', width: 300, height: 140 },
  { id: 'lonely', width: 300, height: 70 },
  { id: 'stray', width: 300, height: 100 },
]
const edges: CardEdge[] = [
  { source: 'root', target: 'left' },
  { source: 'root', target: 'right' },
  { source: 'left', target: 'shared' },
  { source: 'right', target: 'shared' },
  { source: 'right', target: 'lonely' },
]

function box(card: CardSize, at: Point) {
  return { left: at.x, top: at.y, right: at.x + card.width, bottom: at.y + card.height }
}

describe('layoutCards', () => {
  test('places every card', async () => {
    const positions = await layoutCards(cards, edges)

    expect([...positions.keys()].sort()).toEqual(cards.map((card) => card.id).sort())
  })

  test('never overlaps two cards', async () => {
    const positions = await layoutCards(cards, edges)
    const boxes = cards.map((card) => box(card, positions.get(card.id) as Point))

    for (const [index, a] of boxes.entries()) {
      for (const b of boxes.slice(index + 1)) {
        const apart = a.right <= b.left || b.right <= a.left || a.bottom <= b.top || b.bottom <= a.top
        expect(apart).toBe(true)
      }
    }
  })

  test('puts every child wholly below its parent, however tall the parent', async () => {
    const positions = await layoutCards(cards, edges)
    const size = new Map(cards.map((card) => [card.id, card]))

    for (const edge of edges) {
      const parent = box(size.get(edge.source) as CardSize, positions.get(edge.source) as Point)
      const child = positions.get(edge.target) as Point
      expect(child.y).toBeGreaterThanOrEqual(parent.bottom)
    }
  })

  test('keeps siblings in the order their parent declares them', async () => {
    const positions = await layoutCards(cards, edges)

    expect((positions.get('left') as Point).x).toBeLessThan((positions.get('right') as Point).x)
  })

  test('gives the same positions for the same input', async () => {
    const first = await layoutCards(cards, edges)
    const second = await layoutCards(cards, edges)

    expect([...second]).toEqual([...first])
  })

  test('ignores an edge to a card that is not shown', async () => {
    const positions = await layoutCards(cards.slice(0, 2), edges)

    expect([...positions.keys()].sort()).toEqual(['left', 'root'])
  })

  test('lays out nothing as nothing', async () => {
    expect((await layoutCards([], [])).size).toBe(0)
  })
})
