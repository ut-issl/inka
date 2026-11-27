import { describe, expect, test } from 'vitest'

import { edge, node, snapshot } from '../test/factory'
import { viewOf } from '../model/view'
import { allExpanded, graphRoots, revealing, toggled, visibleNodes } from './visibility'

/**
 * root ─┬─ left ──┐
 *       └─ right ─┴─ shared ── deep
 *            └─ lonely
 * plus `stray ── under`, which no objective reaches.
 */
const page = snapshot([
  node('root', { mode: 'objective', root: true }),
  node('left'),
  node('right'),
  node('shared'),
  node('lonely', { mode: 'fiat' }),
  node('deep', { mode: 'fiat' }),
  node('stray'),
  node('under', { mode: 'fiat' }),
  edge('root', 'left'),
  edge('root', 'right'),
  edge('left', 'shared'),
  edge('right', 'shared'),
  edge('right', 'lonely'),
  edge('shared', 'deep'),
  edge('stray', 'under'),
])
const nodes = viewOf(page).nodes
const roots = graphRoots(page)
const shown = (...expanded: string[]) => visibleNodes(nodes, roots, new Set(expanded))

describe('graphRoots', () => {
  test('starts from the objectives, then the topmost nodes no objective reaches', () => {
    expect(roots).toEqual(['root', 'stray'])
  })
})

describe('visibleNodes', () => {
  test('with nothing expanded, shows the roots alone', () => {
    expect(shown()).toEqual(['root', 'stray'])
  })

  test('expanding a node shows its direct children and nothing deeper', () => {
    expect(shown('root')).toEqual(['root', 'stray', 'left', 'right'])
  })

  test('shows a node with several parents once, when any of them is expanded', () => {
    expect(shown('root', 'left').filter((id) => id === 'shared')).toHaveLength(1)
    expect(shown('root', 'left', 'right').filter((id) => id === 'shared')).toHaveLength(1)
  })

  test('collapsing one parent keeps a child another expanded parent still shows', () => {
    const both = shown('root', 'left', 'right', 'shared')
    const afterCollapse = shown('root', 'right', 'shared')

    expect(both).toContain('deep')
    expect(afterCollapse).toContain('shared')
    expect(afterCollapse).toContain('deep')
    expect(shown('root', 'shared')).not.toContain('shared')
  })

  test('an expanded node that is itself hidden shows nothing', () => {
    expect(shown('shared')).toEqual(['root', 'stray'])
  })

  test('depends on what is expanded, never on the order it was expanded in', () => {
    const forwards = ['root', 'left', 'shared', 'right']
    const backwards = [...forwards].reverse()
    const grow = (order: string[]) =>
      order.reduce<Set<string>>((expanded, id) => toggled(expanded, id), new Set<string>())

    expect(visibleNodes(nodes, roots, grow(forwards))).toEqual(visibleNodes(nodes, roots, grow(backwards)))
  })

  test('shows every node, each once, when everything is expanded', () => {
    const all = visibleNodes(nodes, roots, allExpanded(nodes))

    expect([...all].sort()).toEqual([...nodes.keys()].sort())
    expect(new Set(all).size).toBe(all.length)
  })

  test('draws what no objective reaches like the rest, from its own root', () => {
    expect(shown('stray')).toEqual(['root', 'stray', 'under'])
  })
})

describe('toggled', () => {
  test('expands a collapsed node and collapses an expanded one, leaving the rest', () => {
    expect([...toggled(new Set(['root']), 'left')].sort()).toEqual(['left', 'root'])
    expect([...toggled(new Set(['root', 'left']), 'left')]).toEqual(['root'])
  })
})

describe('revealing', () => {
  test('expands a path from a root down to the node, and closes nothing', () => {
    const opened = revealing(nodes, new Set(['stray']), 'deep')

    expect(visibleNodes(nodes, roots, opened)).toContain('deep')
    expect(opened.has('stray')).toBe(true)
    // One path, through the first parent at each step: not the whole graph.
    expect([...opened].sort()).toEqual(['left', 'root', 'shared', 'stray'])
  })

  test('needs nothing to reveal a root', () => {
    expect([...revealing(nodes, new Set(), 'root')]).toEqual([])
  })
})
