import { describe, expect, test } from 'vitest'

import { edge, node } from '../test/factory'
import { parseElements } from './elements'
import { buildView } from './view'
import { isNamespaceFocused, isNodeFocused, resolveFocus, WHOLE_VIEW } from './focus'

/**
 *      top/a
 *      /    \
 *   mid/b   mid/c
 *      |      |
 *   low/d   low/e
 */
const view = buildView(
  parseElements([
    node('a', { mode: 'objective', namespace: 'top', root: true }),
    node('b', { namespace: 'mid' }),
    node('c', { namespace: 'mid' }),
    node('d', { mode: 'fiat', namespace: 'low' }),
    node('e', { mode: 'fiat', namespace: 'low' }),
    edge('a', 'b'),
    edge('a', 'c'),
    edge('b', 'd'),
    edge('c', 'e'),
  ]),
)

const ids = (set: ReadonlySet<string>) => [...set].sort()

describe('resolveFocus', () => {
  test('the whole view shows every node and highlights none', () => {
    const sets = resolveFocus(view, WHOLE_VIEW)

    expect(sets.visible).toBeNull()
    expect(sets.primary.size).toBe(0)
    expect(sets.parents.size).toBe(0)
    expect(sets.children.size).toBe(0)
  })

  test('a node focus keeps the node with its direct parents and children only', () => {
    const sets = resolveFocus(view, { kind: 'node', id: 'b' })

    expect(ids(sets.primary)).toEqual(['b'])
    expect(ids(sets.parents)).toEqual(['a'])
    expect(ids(sets.children)).toEqual(['d'])
    expect(ids(sets.visible as ReadonlySet<string>)).toEqual(['a', 'b', 'd'])
  })

  test('a namespace focus keeps its members plus everything one hop away', () => {
    const sets = resolveFocus(view, { kind: 'namespace', name: 'mid' })

    expect(ids(sets.primary)).toEqual(['b', 'c'])
    expect(ids(sets.parents)).toEqual(['a'])
    expect(ids(sets.children)).toEqual(['d', 'e'])
    expect(ids(sets.visible as ReadonlySet<string>)).toEqual(['a', 'b', 'c', 'd', 'e'])
  })

  test('a member of the focused namespace is never counted as a neighbour', () => {
    const sets = resolveFocus(view, { kind: 'namespace', name: 'low' })

    expect(ids(sets.parents)).toEqual(['b', 'c'])
    expect(sets.children.size).toBe(0)
  })

  test('a node both above and below a namespace reads as a parent', () => {
    // Arrange: `x` is a parent of ns/m and a child of ns/n.
    const both = buildView(
      parseElements([
        node('m', { namespace: 'ns' }),
        node('n', { namespace: 'ns' }),
        node('x', { namespace: 'other' }),
        edge('x', 'm'),
        edge('n', 'x'),
      ]),
    )

    const sets = resolveFocus(both, { kind: 'namespace', name: 'ns' })

    expect(ids(sets.parents)).toEqual(['x'])
    expect(sets.children.size).toBe(0)
  })
})

describe('focus predicates', () => {
  test('identify the focused node and namespace', () => {
    expect(isNodeFocused({ kind: 'node', id: 'b' }, 'b')).toBe(true)
    expect(isNodeFocused({ kind: 'node', id: 'b' }, 'c')).toBe(false)
    expect(isNamespaceFocused({ kind: 'namespace', name: 'mid' }, 'mid')).toBe(true)
    expect(isNamespaceFocused(WHOLE_VIEW, 'mid')).toBe(false)
  })
})
