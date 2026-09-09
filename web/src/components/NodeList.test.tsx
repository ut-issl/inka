import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, test, vi } from 'vitest'

import { edge, node } from '../test/factory'
import { namespaceColors } from '../graph/palette'
import { parseElements } from '../model/elements'
import { resolveFocus, WHOLE_VIEW, type Focus } from '../model/focus'
import { buildView, namespacesOf } from '../model/view'
import { NodeList } from './NodeList'

const view = buildView(
  parseElements([
    node('a', { mode: 'objective', namespace: 'top', root: true }),
    node('b', { namespace: 'mid' }),
    node('c', { namespace: 'mid' }),
    node('d', { mode: 'fiat', namespace: 'low' }),
    edge('a', 'b'),
    edge('a', 'c'),
    edge('b', 'd'),
  ]),
)

function renderList(focus: Focus = WHOLE_VIEW) {
  const onFocusNode = vi.fn()
  const onFocusNamespace = vi.fn()
  render(
    <NodeList
      view={view}
      focus={focus}
      sets={resolveFocus(view, focus)}
      colors={namespaceColors(namespacesOf(view))}
      onFocusNode={onFocusNode}
      onFocusNamespace={onFocusNamespace}
    />,
  )
  return { onFocusNode, onFocusNamespace }
}

describe('NodeList', () => {
  test('lists every node under its namespace, in the view order', () => {
    renderList()

    const entries = screen.getAllByRole('button').map((button) => button.textContent)
    expect(entries).toEqual(['top', 'a', 'mid', 'b', 'c', 'low', 'd'])
  })

  test('focuses a node when its entry is clicked', async () => {
    const { onFocusNode } = renderList()

    await userEvent.click(screen.getByRole('button', { name: 'b' }))

    expect(onFocusNode).toHaveBeenCalledWith('b')
  })

  test('focuses a namespace when its header is clicked', async () => {
    const { onFocusNamespace } = renderList()

    await userEvent.click(screen.getByRole('button', { name: 'mid' }))

    expect(onFocusNamespace).toHaveBeenCalledWith('mid')
  })

  test('marks the focused node, its parents and its children apart', () => {
    renderList({ kind: 'node', id: 'b' })

    expect(screen.getByRole('button', { name: 'b' })).toHaveClass('is-focused')
    expect(screen.getByRole('button', { name: 'a' })).toHaveClass('is-parent')
    expect(screen.getByRole('button', { name: 'd' })).toHaveClass('is-child')
    expect(screen.getByRole('button', { name: 'c' })).not.toHaveClass('is-parent', 'is-child')
  })

  test('emphasizes a focused namespace and its members', () => {
    renderList({ kind: 'namespace', name: 'mid' })

    expect(screen.getByRole('button', { name: 'mid' })).toHaveClass('is-focused')
    expect(screen.getByRole('button', { name: 'b' })).toHaveClass('is-focused')
    expect(screen.getByRole('button', { name: 'a' })).toHaveClass('is-parent')
    expect(screen.getByRole('button', { name: 'd' })).toHaveClass('is-child')
  })

  test('gives each namespace group its own colour', () => {
    renderList()

    const headers = screen.getAllByRole('button').filter((button) => button.className.includes('header'))
    const colours = headers.map((header) => header.style.borderLeftColor)
    expect(new Set(colours).size).toBe(headers.length)
  })

  test('shows nothing that could change the argument', () => {
    renderList({ kind: 'node', id: 'b' })

    const group = screen.getByRole('button', { name: 'a' }).closest('section') as HTMLElement
    expect(within(group).queryAllByRole('link')).toHaveLength(0)
    expect(within(group).queryAllByRole('textbox')).toHaveLength(0)
  })
})
