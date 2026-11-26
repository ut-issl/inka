import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { describe, expect, test } from 'vitest'

import type { Snapshot } from '../generated/Snapshot'
import { viewOf } from '../model/view'
import type { Surface } from '../paths'
import { edge, node, snapshot } from '../test/factory'
import { Sidebar } from './Sidebar'

function page(): Snapshot {
  const built = snapshot([
    node('root', { mode: 'objective', root: true, namespace: 'top', description: 'The spacecraft points.' }),
    node('budget', { namespace: 'pointing', description: 'The pointing budget closes with margin.' }),
    node('sensor', { mode: 'fiat', namespace: 'pointing', description: 'The star tracker meets its accuracy.' }),
    edge('root', 'budget'),
    edge('budget', 'sensor'),
  ])
  built.nodes.find((candidate) => candidate.id === 'sensor')!.status = { state: 'established' }
  built.nodes.find((candidate) => candidate.id === 'sensor')!.review = null
  built.queue = { walk: ['budget', 'root'], groups: [{ state: 'unapproved', targets: ['budget', 'root'] }] }
  return built
}

function renderSidebar(surface: Surface, currentId?: string) {
  const built = page()
  render(
    <MemoryRouter>
      <Sidebar snapshot={built} nodes={viewOf(built).nodes} surface={surface} currentId={currentId} />
    </MemoryRouter>,
  )
}

describe('Sidebar', () => {
  test('lists the review queue and every node of the argument', () => {
    renderSidebar('review')

    const queue = screen.getByRole('navigation', { name: 'Review queue' })
    expect(within(queue).getAllByRole('link')).toHaveLength(2)
    const all = screen.getByRole('navigation', { name: 'All nodes' })
    expect(within(all).getByRole('heading')).toHaveTextContent('All nodes (3)')
    expect(within(all).getAllByRole('link').map((link) => link.querySelector('.queue__id')?.textContent)).toEqual([
      'root',
      'budget',
      'sensor',
    ])
  })

  test('shows each node’s mode, namespace and status, in words', () => {
    renderSidebar('review')

    const entry = within(screen.getByRole('navigation', { name: 'All nodes' })).getByRole('link', { name: /sensor/ })
    expect(entry).toHaveTextContent('fiat:sensor')
    expect(entry).toHaveTextContent('pointing')
    expect(entry).toHaveTextContent('established')
  })

  test('opens a node on the surface the sidebar is on', () => {
    renderSidebar('graph')

    const all = screen.getByRole('navigation', { name: 'All nodes' })
    expect(within(all).getByRole('link', { name: /budget/ })).toHaveAttribute('href', '/graph/budget')
    const queue = screen.getByRole('navigation', { name: 'Review queue' })
    expect(within(queue).getByRole('link', { name: /budget/ })).toHaveAttribute('href', '/graph/budget')
  })

  test('marks the node the surface is showing', () => {
    renderSidebar('review', 'budget')

    const all = screen.getByRole('navigation', { name: 'All nodes' })
    expect(within(all).getByRole('link', { name: /budget/ })).toHaveAttribute('aria-current', 'true')
    expect(within(all).getByRole('link', { name: /sensor/ })).not.toHaveAttribute('aria-current')
  })

  test('finds nodes by id', async () => {
    const user = userEvent.setup()
    renderSidebar('review')

    await user.type(screen.getByRole('searchbox'), 'SENS')

    const results = screen.getByRole('navigation', { name: 'Search results' })
    expect(within(results).getByRole('heading')).toHaveTextContent('1 of 3 nodes match')
    expect(within(results).getAllByRole('link')).toHaveLength(1)
    expect(within(results).getByRole('link')).toHaveTextContent('fiat:sensor')
  })

  test('finds nodes by the text of their description', async () => {
    const user = userEvent.setup()
    renderSidebar('review')

    await user.type(screen.getByRole('searchbox'), 'pointing budget')

    const results = screen.getByRole('navigation', { name: 'Search results' })
    expect(within(results).getAllByRole('link').map((link) => link.querySelector('.queue__id')?.textContent)).toEqual([
      'budget',
    ])
  })

  test('says so when nothing matches, and gives the lists back when the search is cleared', async () => {
    const user = userEvent.setup()
    renderSidebar('review')

    await user.type(screen.getByRole('searchbox'), 'thermal')
    expect(screen.getByText('No node matches')).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: 'Review queue' })).toBeNull()

    await user.clear(screen.getByRole('searchbox'))
    expect(screen.getByRole('navigation', { name: 'Review queue' })).toBeInTheDocument()
  })

  test('offers no decision on any entry', () => {
    renderSidebar('review')

    expect(screen.queryByRole('button')).toBeNull()
  })
})
