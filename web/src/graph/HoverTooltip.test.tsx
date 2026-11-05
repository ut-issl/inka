import { act, render, screen } from '@testing-library/react'
import cytoscape from 'cytoscape'
import { describe, expect, test, vi } from 'vitest'

import { parseElements } from '../model/elements'
import { buildView } from '../model/view'
import { edge, node } from '../test/factory'
import { cytoscapeElements } from './elements'
import { HoverTooltip } from './HoverTooltip'

const view = buildView(
  parseElements([
    node('a', { description: 'the parent contract' }),
    node('b'),
    node('c'),
    edge('a', 'b', 'why b is here'),
    edge('a', 'c'),
  ]),
)

/** A headless graph of `view` with the tooltip mounted over it. */
function mounted() {
  const cy = cytoscape({ headless: true, elements: cytoscapeElements(view) })
  const result = render(<HoverTooltip cy={cy} view={view} />)
  const emit = (id: string, event: 'mouseover' | 'mouseout') =>
    act(() => {
      cy.getElementById(id).emit(event)
    })
  return { cy, emit, ...result }
}

describe('HoverTooltip', () => {
  test('follows the pointer from a node onto a commented edge and off it', () => {
    const { emit } = mounted()

    emit('a', 'mouseover')
    expect(screen.getByRole('tooltip')).toHaveTextContent('the parent contract')

    emit('a', 'mouseout')
    emit('a->b', 'mouseover')
    expect(screen.getByRole('tooltip')).toHaveTextContent('a → b')
    expect(screen.getByRole('tooltip')).toHaveTextContent('why b is here')

    emit('a->b', 'mouseout')
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument()
  })

  test('an edge without a comment shows no tooltip', () => {
    const { emit } = mounted()

    emit('a->c', 'mouseover')

    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument()
  })

  test('removes exactly the handlers it registered once unmounted', () => {
    const { cy, unmount } = mounted()
    const off = vi.spyOn(cy, 'off')

    unmount()

    const removed = off.mock.calls.map(([events, selector]) => `${String(events)} ${String(selector)}`)
    expect(removed).toEqual(
      expect.arrayContaining(['mouseover node', 'mouseover edge', 'mouseout node, edge']),
    )
  })
})
