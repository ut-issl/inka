import { describe, expect, test } from 'vitest'

import { edge, node } from '../test/factory'
import { parseElements } from '../model/elements'
import { buildView } from '../model/view'
import { cytoscapeElements } from './elements'

const view = buildView(
  parseElements([
    node('a', { mode: 'objective', namespace: 'top', description: 'secret contract' }),
    node('b', { mode: 'fiat', namespace: 'low' }),
    node('c', { mode: 'fiat', namespace: 'low' }),
    edge('a', 'b'),
    edge('a', 'c', 'secret entry comment'),
  ]),
)

describe('cytoscapeElements', () => {
  test('emits one node per block, in the view order', () => {
    const nodes = cytoscapeElements(view).filter((element) => element.group === 'nodes')

    expect(nodes.map((element) => element.data.id)).toEqual(view.order)
  })

  test('carries only what the stylesheet selects on', () => {
    const [first] = cytoscapeElements(view)

    expect(first?.data).toEqual({ id: 'a', mode: 'objective', namespace: 'top' })
  })

  test('never hands a description to the graph, so none can be drawn in a node', () => {
    const serialized = JSON.stringify(cytoscapeElements(view))

    expect(serialized).not.toContain('secret contract')
  })

  test('emits each edge from the parent to the child', () => {
    const edges = cytoscapeElements(view).filter((element) => element.group === 'edges')

    expect(edges.map((element) => element.data)).toEqual([
      { id: 'a->b', source: 'a', target: 'b', commented: false },
      { id: 'a->c', source: 'a', target: 'c', commented: true },
    ])
  })

  test('flags a commented entry without handing its text to the graph', () => {
    const serialized = JSON.stringify(cytoscapeElements(view))

    expect(serialized).not.toContain('secret entry comment')
  })
})
