import { render, screen } from '@testing-library/react'
import { describe, expect, test } from 'vitest'

import { edge, node } from '../test/factory'
import { resolveFocus, WHOLE_VIEW, type Focus } from '../model/focus'
import { parseElements } from '../model/elements'
import { buildView } from '../model/view'
import { DetailPanel } from './DetailPanel'

const view = buildView(
  parseElements([
    node('a', {
      mode: 'objective',
      namespace: 'top',
      root: true,
      description: 'the objective',
      comments: { children: 'one per branch' },
    }),
    node('b', {
      mode: 'bind',
      namespace: 'mid',
      description: 'the contract text',
      tags: ['safety'],
      evidence: { kind: 'veriq', target: 'pkg.verify_thing' },
      deadline: { until: 'FM', promote: 'bind' },
      comments: {
        block: 'shared with the thermal argument',
        veriq: 'the shipped check',
        until: 'revisit at CDR\nonce the model lands',
        promote: 'once the rig exists',
      },
    }),
    node('c', { mode: 'fiat', namespace: 'mid' }),
    edge('a', 'b', 'why b is listed'),
    edge('a', 'c'),
  ]),
)

function renderPanel(focus: Focus) {
  render(<DetailPanel view={view} focus={focus} sets={resolveFocus(view, focus)} />)
}

describe('DetailPanel', () => {
  test('shows the selected node contract, evidence target and deadline', () => {
    renderPanel({ kind: 'node', id: 'b' })

    expect(screen.getByRole('heading', { name: 'b' })).toBeInTheDocument()
    expect(screen.getByText('the contract text')).toBeInTheDocument()
    expect(screen.getByText('pkg.verify_thing')).toBeInTheDocument()
    expect(screen.getByText(/FM/)).toBeInTheDocument()
  })

  test('shows each comment with the element it is attached to', () => {
    renderPanel({ kind: 'node', id: 'b' })

    const rowOf = (comment: RegExp) => screen.getByText(comment).closest('p, div')
    expect(screen.getByText(/shared with the thermal argument/)).toHaveClass('detail__comment')
    expect(rowOf(/the shipped check/)).toHaveTextContent(/Verified by/)
    expect(rowOf(/revisit at CDR/)).toHaveTextContent(/^Until FM/)
    // The line break survives: the text is rendered as written, not reflowed.
    expect(screen.getByText(/revisit at CDR/).textContent).toBe(
      'Comment: revisit at CDR\nonce the model lands',
    )
    expect(rowOf(/once the rig exists/)).toHaveTextContent(/^Promote to bind/)
  })

  test('labels every comment for assistive technology', () => {
    renderPanel({ kind: 'node', id: 'b' })

    for (const comment of document.querySelectorAll('.detail__comment')) {
      expect(comment.querySelector('.visually-hidden')).toHaveTextContent('Comment:')
    }
  })

  test('lists each child under the children property comment, with its entry comment', () => {
    renderPanel({ kind: 'node', id: 'a' })

    const row = screen.getByText(/one per branch/).closest('div')
    expect(row).toHaveTextContent(/^Children/)
    const items = screen.getAllByRole('listitem')
    expect(items.map((item) => item.firstChild?.textContent)).toEqual(['b', 'c'])
    expect(items[0]).toHaveTextContent(/why b is listed/)
    expect(items[1]?.querySelector('.detail__comment')).toBeNull()
  })

  test('omits evidence and deadline from a node that declares neither', () => {
    renderPanel({ kind: 'node', id: 'c' })

    expect(screen.queryByText(/Verified by/)).not.toBeInTheDocument()
    expect(screen.queryByText(/Until/)).not.toBeInTheDocument()
    expect(screen.queryByText(/Children/)).not.toBeInTheDocument()
  })

  test('summarises a focused namespace and its one-hop surroundings', () => {
    renderPanel({ kind: 'namespace', name: 'mid' })

    expect(screen.getByRole('heading', { name: 'mid' })).toBeInTheDocument()
    expect(screen.getByText(/2 nodes in this namespace/)).toBeInTheDocument()
    expect(screen.getByText(/1 one hop away/)).toBeInTheDocument()
  })

  test('names the objective roots in the whole view', () => {
    renderPanel(WHOLE_VIEW)

    expect(screen.getByText(/Objective on top: a/)).toBeInTheDocument()
    expect(screen.getByText(/3 nodes · 2 edges · 2 namespaces/)).toBeInTheDocument()
  })

  test('offers no action that would change the argument', () => {
    renderPanel({ kind: 'node', id: 'b' })

    expect(screen.queryAllByRole('button')).toHaveLength(0)
    expect(screen.queryAllByRole('link')).toHaveLength(0)
  })
})
