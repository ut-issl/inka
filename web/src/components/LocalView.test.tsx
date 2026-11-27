import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, test } from 'vitest'

import type { NodeData } from '../generated/NodeData'
import { edge, node, snapshot } from '../test/factory'
import { LocalView } from './LocalView'

const page = snapshot([
  node('parent', {
    mode: 'structure',
    description: 'The pointing budget **closes**.',
    comments: { block: 'shared with the thermal argument', children: 'one per error source' },
    deadline: { until: 'FM', promote: 'bind' },
  }),
  node('zeta', { description: 'zeta contract' }),
  node('alpha', { description: 'alpha contract' }),
  node('deep', { mode: 'fiat', description: 'never shown from the parent' }),
  edge('parent', 'zeta'),
  edge('parent', 'alpha', 'the dominant term'),
  edge('zeta', 'deep'),
])
const parent = page.nodes.find((candidate) => candidate.id === 'parent')!

describe('LocalView', () => {
  test('names the target with its status and review state, in words', () => {
    render(<LocalView node={parent} />)

    expect(screen.getByRole('heading', { name: 'structure:parent' })).toBeInTheDocument()
    expect(screen.getByText('blamed')).toBeInTheDocument()
    expect(screen.getByText('unapproved')).toBeInTheDocument()
    expect(screen.getByText('no approval covers the current content')).toBeInTheDocument()
  })

  test('shows the content in the order `inka review` prints it', () => {
    render(<LocalView node={parent} />)

    const contract = screen.getByRole('region', { name: 'Contract content' })
    const text = contract.textContent ?? ''
    const order = [
      '// shared with the thermal argument',
      'The pointing budget closes.',
      '// one per error source',
      'children:',
      'zeta contract',
      '// the dominant term',
      'alpha contract',
      'until:',
      'FM',
      'promote:',
      'bind',
    ].map((piece) => text.indexOf(piece))

    expect(order.every((position) => position >= 0)).toBe(true)
    expect([...order].sort((a, b) => a - b)).toEqual(order)
  })

  test('lists the direct children in declaration order, each under the comment on its entry', () => {
    render(<LocalView node={parent} />)

    const children = within(screen.getByRole('region', { name: 'Contract content' })).getAllByRole('listitem')
    expect(children.map((child) => child.textContent)).toEqual([
      expect.stringContaining('zeta'),
      expect.stringContaining('// the dominant term'),
    ])
  })

  test('shows nothing deeper than the direct children', () => {
    render(<LocalView node={parent} />)

    expect(screen.queryByText('never shown from the parent')).toBeNull()
    expect(screen.queryByText('deep')).toBeNull()
  })

  test('shows a bind’s evidence reference under the comment on its own property', () => {
    const bound = snapshot([
      node('bound', {
        mode: 'bind',
        evidence: { kind: 'test', target: 'tests/hash.rs::golden' },
        comments: { test: 'the golden vectors' },
      }),
    ]).nodes[0]!

    render(<LocalView node={bound} />)

    const contract = screen.getByRole('region', { name: 'Contract content' }).textContent ?? ''
    expect(contract.indexOf('// the golden vectors')).toBeLessThan(contract.indexOf('test:'))
    expect(screen.getByText('"tests/hash.rs::golden"')).toBeInTheDocument()
  })

  test('puts the source of a description one control away from its rendering', async () => {
    const user = userEvent.setup()
    const spaced = snapshot([node('spaced', { description: 'one\ntwo   three' })]).nodes[0]!
    render(<LocalView node={spaced} />)
    expect(screen.getByText('one two three')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'Source' }))

    expect(document.querySelector('pre')?.textContent).toBe('one\ntwo   three')
    expect(screen.getByRole('button', { name: 'Rendered' })).toHaveAttribute('aria-pressed', 'true')
  })

  test('says where each child stands, and why a blamed one is blamed', () => {
    const nodes = new Map(page.nodes.map((candidate) => [candidate.id, candidate]))
    nodes.set('zeta', {
      ...nodes.get('zeta')!,
      status: { state: 'blamed', reasons: [{ cause: 'missing_evidence', target: 'tests/zeta.rs' }] },
    })
    nodes.set('alpha', { ...nodes.get('alpha')!, status: { state: 'unestablished' }, blamed_below: 2 })

    render(<LocalView node={parent} nodes={nodes} />)

    const [zeta, alpha] = within(screen.getByRole('region', { name: 'Contract content' })).getAllByRole('listitem')
    expect(within(zeta!).getByText('blamed')).toBeInTheDocument()
    expect(within(zeta!).getByText('no imported test evidence provides `tests/zeta.rs`')).toBeInTheDocument()
    expect(within(alpha!).getByText('not established')).toBeInTheDocument()
    expect(within(alpha!).getByText('2 nodes below it are blamed')).toBeInTheDocument()
  })

  test('says who declined a structure child and not why, and quotes the comment on any other', () => {
    const declined: NodeData['status'] = {
      state: 'blamed',
      reasons: [{ cause: 'declined', by: 'sato', comment: 'the split is wrong' }],
    }
    const nodes = new Map(page.nodes.map((candidate) => [candidate.id, candidate]))
    nodes.set('zeta', { ...nodes.get('zeta')!, mode: 'structure', status: declined })
    nodes.set('alpha', { ...nodes.get('alpha')!, mode: 'fiat', status: declined })

    render(<LocalView node={parent} nodes={nodes} />)

    const [zeta, alpha] = within(screen.getByRole('region', { name: 'Contract content' })).getAllByRole('listitem')
    expect(within(zeta!).getByText('declined by sato')).toBeInTheDocument()
    expect(within(alpha!).getByText('declined by sato: the split is wrong')).toBeInTheDocument()
  })

  test('gives an established child its status and nothing more', () => {
    const nodes = new Map(page.nodes.map((candidate) => [candidate.id, candidate]))
    nodes.set('zeta', { ...nodes.get('zeta')!, status: { state: 'established' } })

    render(<LocalView node={parent} nodes={nodes} />)

    const [zeta] = within(screen.getByRole('region', { name: 'Contract content' })).getAllByRole('listitem')
    expect(within(zeta!).getByText('established')).toBeInTheDocument()
    expect(zeta!.querySelector('.local__child-status')).toBeNull()
  })

  test('draws a reference to a child the way the surface asks', () => {
    render(<LocalView node={parent} reference={(id) => <a href={`#${id}`}>{id}</a>} />)

    expect(screen.getByRole('link', { name: 'alpha' })).toHaveAttribute('href', '#alpha')
  })
})
