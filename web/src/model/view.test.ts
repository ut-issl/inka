import { describe, expect, test } from 'vitest'

import { edge, node, view } from '../test/factory'
import { nodeOf, ViewError } from './view'

/** A diamond: a → b, a → c, b → d, c → d. */
function diamond() {
  return view([
    node('a', { mode: 'objective', root: true }),
    node('b'),
    node('c'),
    node('d', { mode: 'fiat' }),
    edge('a', 'b'),
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

  test('passes the order through as the server sent it', () => {
    expect(diamond().order).toEqual(['a', 'b', 'c', 'd'])
  })

  test('fails loudly when an unknown node is looked up', () => {
    expect(() => nodeOf(diamond(), 'ghost')).toThrow(ViewError)
  })
})
