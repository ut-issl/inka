import { describe, expect, test } from 'vitest'

import { edge, node, view } from '../test/factory'
import { namespacesOf, nodeOf, rootsOf, ViewError } from './view'

/** A diamond: a → b, a → c, b → d, c → d. */
function diamond() {
  return view([
    node('a', { mode: 'objective', namespace: 'top', root: true }),
    node('b', { namespace: 'left' }),
    node('c', { namespace: 'right' }),
    node('d', { mode: 'fiat', namespace: 'left' }),
    edge('a', 'b', 'the left branch'),
    edge('a', 'c'),
    edge('b', 'd'),
    edge('c', 'd'),
  ])
}

describe('viewOf', () => {
  test('indexes every node by id, one entry each', () => {
    const view = diamond()

    expect([...view.nodes.keys()]).toEqual(['a', 'b', 'c', 'd'])
    expect(nodeOf(view, 'd').parents).toEqual(['b', 'c'])
  })

  test('passes the order and the groups through as the server sent them', () => {
    const view = diamond()

    expect(view.order).toEqual(['a', 'b', 'c', 'd'])
    expect(view.groups).toEqual([
      { name: 'top', nodes: ['a'] },
      { name: 'left', nodes: ['b', 'd'] },
      { name: 'right', nodes: ['c'] },
    ])
    expect(namespacesOf(view)).toEqual(['top', 'left', 'right'])
  })

  test('lays the entries out parent by parent, in declaration order, with their comments', () => {
    const view = diamond()

    expect(view.edges).toEqual([
      { id: 'a->b', source: 'a', target: 'b', comment: 'the left branch' },
      { id: 'a->c', source: 'a', target: 'c', comment: null },
      { id: 'b->d', source: 'b', target: 'd', comment: null },
      { id: 'c->d', source: 'c', target: 'd', comment: null },
    ])
  })

  test('reports the objective roots in view order', () => {
    expect(rootsOf(diamond()).map((root) => root.id)).toEqual(['a'])
  })

  test('fails loudly when an unknown node is looked up', () => {
    expect(() => nodeOf(diamond(), 'ghost')).toThrow(ViewError)
  })
})
