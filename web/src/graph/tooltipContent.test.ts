import { describe, expect, test } from 'vitest'

import { parseElements } from '../model/elements'
import { buildView } from '../model/view'
import { edge, node } from '../test/factory'
import { tooltipContent } from './tooltipContent'

const view = buildView(
  parseElements([
    node('a', { description: 'the parent contract' }),
    node('b'),
    node('c'),
    edge('a', 'b', 'why b is here'),
    edge('a', 'c'),
  ]),
)

describe('tooltipContent', () => {
  test('a node shows its id and its contract', () => {
    expect(tooltipContent(view, { kind: 'node', id: 'a' })).toEqual({
      title: 'a',
      text: 'the parent contract',
    })
  })

  test('a commented edge shows its entry and the comment on it', () => {
    expect(tooltipContent(view, { kind: 'edge', id: 'a->b' })).toEqual({
      title: 'a → b',
      text: 'why b is here',
    })
  })

  test('an edge without a comment shows nothing', () => {
    expect(tooltipContent(view, { kind: 'edge', id: 'a->c' })).toBeNull()
  })

  test('an unknown element shows nothing', () => {
    expect(tooltipContent(view, { kind: 'node', id: 'absent' })).toBeNull()
  })
})
