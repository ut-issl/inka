import { describe, expect, test } from 'vitest'

import {
  boundingBox,
  egoRows,
  namespaceOrder,
  orderRanks,
  placeRow,
  ranksOf,
  stretchToAspect,
  type LayoutNode,
} from './layout'

function box(id: string, namespace: string, x: number, y: number, width = 100, height = 40): LayoutNode {
  return { id, namespace, position: { x, y }, width, height }
}

/** Aspect ratio of the nodes' bounding box, sizes included. */
function aspectOf(nodes: readonly LayoutNode[]): number {
  const bounds = boundingBox(nodes)
  if (!bounds) return 0
  return (bounds.x2 - bounds.x1) / (bounds.y2 - bounds.y1)
}

function moved(nodes: readonly LayoutNode[], positions: ReadonlyMap<string, { x: number; y: number }>) {
  return nodes.map((node) => ({ ...node, position: positions.get(node.id) ?? node.position }))
}

describe('ranksOf', () => {
  test('groups nodes by row and orders the rows top to bottom', () => {
    const nodes = [box('a', 'x', 0, 100), box('b', 'x', 50, 0), box('c', 'x', 10, 100.4)]

    expect(ranksOf(nodes).map((rank) => rank.map((node) => node.id))).toEqual([['b'], ['a', 'c']])
  })
})

describe('namespaceOrder', () => {
  test('orders namespaces by mean x so bands follow the layered pass', () => {
    const nodes = [box('a', 'right', 300, 0), box('b', 'left', 0, 0), box('c', 'left', 100, 100)]

    expect(namespaceOrder(nodes)).toEqual(['left', 'right'])
  })

  test('breaks a tie on the name, so the order never depends on iteration', () => {
    const nodes = [box('a', 'zeta', 0, 0), box('b', 'alpha', 0, 0)]

    expect(namespaceOrder(nodes)).toEqual(['alpha', 'zeta'])
  })
})

describe('placeRow', () => {
  test('centres the row on x = 0 and leaves the gap between boxes', () => {
    const nodes = [box('a', 'x', 0, 0, 100), box('b', 'x', 0, 0, 200)]

    const placed = placeRow(nodes, 7, 40)

    expect(placed.get('a')).toEqual({ x: -120, y: 7 })
    expect(placed.get('b')).toEqual({ x: 70, y: 7 })
  })

  test('never overlaps two boxes, whatever their widths', () => {
    const nodes = [box('a', 'x', 0, 0, 250), box('b', 'x', 0, 0, 30), box('c', 'x', 0, 0, 120)]

    const placed = moved(nodes, placeRow(nodes, 0, 10))

    for (let i = 1; i < placed.length; i += 1) {
      const left = placed[i - 1] as LayoutNode
      const right = placed[i] as LayoutNode
      expect(right.position.x - right.width / 2).toBeGreaterThanOrEqual(left.position.x + left.width / 2)
    }
  })
})

describe('orderRanks', () => {
  const nodes = [
    box('a1', 'alpha', 300, 0),
    box('b1', 'beta', 0, 0),
    box('a2', 'alpha', 400, 100),
    box('b2', 'beta', 50, 100),
    box('a3', 'alpha', 250, 100),
  ]

  test('makes same-namespace nodes adjacent within every rank', () => {
    const placed = moved(nodes, orderRanks(nodes, 40))

    for (const rank of ranksOf(placed)) {
      const namespaces = [...rank]
        .sort((left, right) => left.position.x - right.position.x)
        .map((node) => node.namespace)
      const distinct = new Set(namespaces)
      // Adjacent means each namespace forms one uninterrupted run.
      expect(namespaces.filter((name, i) => name !== namespaces[i - 1])).toHaveLength(distinct.size)
    }
  })

  test('uses one namespace order across all ranks', () => {
    const placed = moved(nodes, orderRanks(nodes, 40))

    const orders = ranksOf(placed).map((rank) =>
      [...rank]
        .sort((left, right) => left.position.x - right.position.x)
        .map((node) => node.namespace)
        .filter((name, i, all) => name !== all[i - 1]),
    )
    expect(orders).toEqual([
      ['beta', 'alpha'],
      ['beta', 'alpha'],
    ])
  })

  test('leaves every rank at its own y, so the layered flow survives', () => {
    const placed = orderRanks(nodes, 40)

    expect(placed.get('a1')?.y).toBe(0)
    expect(placed.get('a3')?.y).toBe(100)
  })
})

describe('egoRows', () => {
  const focused = box('n', 'mid', 0, 0)
  const rows = {
    parents: [box('p2', 'beta', 0, 0), box('p1', 'alpha', 0, 0)],
    focused,
    children: [box('c1', 'alpha', 0, 0), box('c2', 'beta', 0, 0)],
  }
  const options = { gap: 40, rowGap: 200, namespaces: ['alpha', 'beta', 'mid'] }

  test('puts the parents above, the node in the middle and the children below', () => {
    const placed = egoRows(rows, options)

    expect(placed.get('p1')?.y).toBe(0)
    expect(placed.get('n')).toEqual({ x: 0, y: 200 })
    expect(placed.get('c1')?.y).toBe(400)
  })

  test('orders each row by the shared namespace order', () => {
    const placed = egoRows(rows, options)

    expect((placed.get('p1') as { x: number }).x).toBeLessThan((placed.get('p2') as { x: number }).x)
    expect((placed.get('c1') as { x: number }).x).toBeLessThan((placed.get('c2') as { x: number }).x)
  })

  test('centres a node that has no parents and no children', () => {
    const placed = egoRows({ parents: [], focused, children: [] }, options)

    expect(placed.get('n')).toEqual({ x: 0, y: 200 })
  })
})

describe('stretchToAspect', () => {
  const options = { tolerance: 0.02, iterations: 8 }

  test('matches a wide window by stretching a tall layout sideways', () => {
    const nodes = [box('a', 'x', 0, 0), box('b', 'x', 0, 400), box('c', 'x', 60, 800)]

    const placed = moved(nodes, stretchToAspect(nodes, 2, options))

    expect(aspectOf(placed)).toBeCloseTo(2, 1)
  })

  test('matches a tall window by stretching a wide layout downward', () => {
    const nodes = [box('a', 'x', 0, 0), box('b', 'x', 500, 0), box('c', 'x', 1000, 30)]

    const placed = moved(nodes, stretchToAspect(nodes, 0.5, options))

    expect(aspectOf(placed)).toBeCloseTo(0.5, 1)
  })

  test('only ever stretches, so nodes that were apart cannot close in', () => {
    const nodes = [box('a', 'x', 0, 0), box('b', 'x', 400, 0), box('c', 'x', 200, 300)]

    const placed = stretchToAspect(nodes, 0.4, options)

    const spreadX = Math.abs((placed.get('b') as { x: number }).x - (placed.get('a') as { x: number }).x)
    expect(spreadX).toBeGreaterThanOrEqual(400)
  })

  test('leaves a layout that already matches untouched', () => {
    const nodes = [box('a', 'x', 0, 0), box('b', 'x', 160, 160)]

    const placed = stretchToAspect(nodes, aspectOf(nodes), options)

    expect(placed.get('b')).toEqual({ x: 160, y: 160 })
  })

  test('leaves a single row alone: there is no spread to stretch', () => {
    const nodes = [box('a', 'x', 0, 0), box('b', 'x', 200, 0)]

    const placed = stretchToAspect(nodes, 0.5, options)

    expect(placed.get('a')).toEqual({ x: 0, y: 0 })
    expect(placed.get('b')).toEqual({ x: 200, y: 0 })
  })

  test('leaves a lone node and an unusable target alone', () => {
    const single = [box('a', 'x', 5, 5)]
    expect(stretchToAspect(single, 2, options).get('a')).toEqual({ x: 5, y: 5 })

    const pair = [box('a', 'x', 0, 0), box('b', 'x', 100, 100)]
    expect(stretchToAspect(pair, 0, options).get('b')).toEqual({ x: 100, y: 100 })
  })
})

describe('boundingBox', () => {
  test('includes the nodes own sizes', () => {
    expect(boundingBox([box('a', 'x', 0, 0, 100, 40)])).toEqual({ x1: -50, y1: -20, x2: 50, y2: 20 })
  })

  test('is null without nodes', () => {
    expect(boundingBox([])).toBeNull()
  })
})
