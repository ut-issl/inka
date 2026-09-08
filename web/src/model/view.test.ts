import { describe, expect, test } from 'vitest'

import { edge, node } from '../test/factory'
import { parseElements } from './elements'
import { buildView, namespacesOf, nodeOf, rootsOf, ViewError } from './view'

/** `a` over `b` and `c`; `d` is shared by both, so the graph is a DAG. */
function diamond() {
  return buildView(
    parseElements([
      node('a', { mode: 'objective', namespace: 'top', root: true }),
      node('b', { namespace: 'left' }),
      node('c', { namespace: 'right' }),
      node('d', { mode: 'fiat', namespace: 'left' }),
      edge('a', 'b'),
      edge('a', 'c'),
      edge('b', 'd'),
      edge('c', 'd'),
    ]),
  )
}

describe('buildView', () => {
  test('resolves each edge into a parent and a child', () => {
    const view = diamond()

    expect(nodeOf(view, 'a').children).toEqual(['b', 'c'])
    expect(nodeOf(view, 'd').parents).toEqual(['b', 'c'])
  })

  test('gives a node with several parents one entry, not one per parent', () => {
    const view = diamond()

    expect(view.order.filter((id) => id === 'd')).toHaveLength(1)
    expect(view.nodes.size).toBe(4)
  })

  test('orders nodes topologically, breaking ties by export order', () => {
    const view = diamond()

    expect(view.order).toEqual(['a', 'b', 'c', 'd'])
  })

  test('renders the same order whatever order the edges arrive in', () => {
    // Arrange: the same argument with its edges listed back to front.
    const reversed = buildView(
      parseElements([
        node('a', { mode: 'objective', namespace: 'top', root: true }),
        node('b', { namespace: 'left' }),
        node('c', { namespace: 'right' }),
        node('d', { mode: 'fiat', namespace: 'left' }),
        edge('c', 'd'),
        edge('b', 'd'),
        edge('a', 'c'),
        edge('a', 'b'),
      ]),
    )

    expect(reversed.order).toEqual(diamond().order)
  })

  test('groups by namespace in first-appearance topological order', () => {
    const view = diamond()

    expect(view.groups).toEqual([
      { name: 'top', nodeIds: ['a'] },
      { name: 'left', nodeIds: ['b', 'd'] },
      { name: 'right', nodeIds: ['c'] },
    ])
    expect(namespacesOf(view)).toEqual(['top', 'left', 'right'])
  })

  test('lists every node exactly once across the groups', () => {
    const view = diamond()

    const listed = view.groups.flatMap((group) => group.nodeIds)
    expect([...listed].sort()).toEqual([...view.order].sort())
  })

  test('reports the objective roots in topological order', () => {
    expect(rootsOf(diamond()).map((root) => root.id)).toEqual(['a'])
  })

  test('rejects a duplicate node id', () => {
    expect(() => buildView(parseElements([node('a'), node('a')]))).toThrow(ViewError)
  })

  test('rejects an edge naming an unknown node', () => {
    expect(() => buildView(parseElements([node('a'), edge('a', 'ghost')]))).toThrow(/ghost/)
  })

  test('rejects a cycle rather than dropping nodes from the order', () => {
    const cyclic = parseElements([node('a'), node('b'), edge('a', 'b'), edge('b', 'a')])

    expect(() => buildView(cyclic)).toThrow(/cycle/)
  })

  test('fails loudly when an unknown node is looked up', () => {
    expect(() => nodeOf(diamond(), 'ghost')).toThrow(ViewError)
  })
})
