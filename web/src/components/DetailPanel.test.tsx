import { render, screen } from '@testing-library/react'
import { describe, expect, test } from 'vitest'

import { edge, node } from '../test/factory'
import { resolveFocus, WHOLE_VIEW, type Focus } from '../model/focus'
import { parseElements } from '../model/elements'
import { buildView } from '../model/view'
import { DetailPanel } from './DetailPanel'

const view = buildView(
  parseElements([
    node('a', { mode: 'objective', namespace: 'top', root: true, description: 'the objective' }),
    node('b', {
      mode: 'bind',
      namespace: 'mid',
      description: 'the contract text',
      tags: ['safety'],
      evidence: { kind: 'veriq', target: 'pkg.verify_thing' },
      deadline: { until: 'FM', promote: 'bind' },
    }),
    node('c', { mode: 'fiat', namespace: 'mid' }),
    edge('a', 'b'),
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

  test('omits evidence and deadline from a node that declares neither', () => {
    renderPanel({ kind: 'node', id: 'c' })

    expect(screen.queryByText(/Verified by/)).not.toBeInTheDocument()
    expect(screen.queryByText(/Until/)).not.toBeInTheDocument()
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
