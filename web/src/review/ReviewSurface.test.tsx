import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router'
import { describe, expect, test } from 'vitest'

import aocsPage from '../../fixtures/aocs.page.json'
import type { Snapshot } from '../generated/Snapshot'
import { viewOf } from '../model/view'
import { edge, node, snapshot } from '../test/factory'
import { ReviewSurface } from './ReviewSurface'

function renderAt(path: string, page: Snapshot) {
  render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/review/:id?" element={<ReviewSurface snapshot={page} nodes={viewOf(page).nodes} />} />
      </Routes>
    </MemoryRouter>,
  )
}

/** Three targets to walk, one declined, one blocked. */
function queued(): Snapshot {
  const page = snapshot([
    node('root', { mode: 'objective', root: true }),
    node('leaf'),
    node('other'),
    node('vetoed'),
    node('waiting', { mode: 'bind', evidence: { kind: 'test', target: 'tests/x.rs' } }),
    edge('root', 'leaf'),
    edge('root', 'other'),
    edge('root', 'vetoed'),
    edge('root', 'waiting'),
  ])
  const set = (id: string, review: Snapshot['nodes'][number]['review']) => {
    page.nodes.find((candidate) => candidate.id === id)!.review = review
  }
  set('vetoed', { state: 'declined', by: ['iwata'] })
  set('waiting', { state: 'blocked', reason: { cause: 'missing_evidence', target: 'tests/x.rs' } })
  page.queue = {
    walk: ['leaf', 'other', 'root'],
    groups: [
      { state: 'unapproved', targets: ['leaf', 'other', 'root'] },
      { state: 'declined', targets: ['vetoed'] },
      { state: 'blocked', targets: ['waiting'] },
    ],
  }
  return page
}

describe('ReviewSurface', () => {
  test('lists the queue as `inka review --list` does: a section per state, counted', () => {
    renderAt('/review', queued())

    const queue = screen.getByRole('navigation', { name: 'Review queue' })
    expect(within(queue).getByText('4 targets need review')).toBeInTheDocument()
    expect(within(queue).getAllByRole('heading').map((heading) => heading.textContent)).toEqual([
      'Unapproved (3)',
      'Declined (1)',
      'Blocked (1)',
    ])
    expect(within(queue).getAllByRole('link').map((link) => link.textContent)).toEqual([
      'structure:leaf',
      'structure:other',
      'objective:root',
      expect.stringContaining('structure:vetoed'),
      expect.stringContaining('bind:waiting'),
    ])
  })

  test('says what resolves a declined and a blocked target', () => {
    renderAt('/review', queued())

    const queue = screen.getByRole('navigation', { name: 'Review queue' })
    expect(within(queue).getByText('by iwata (edit or retract to resolve)')).toBeInTheDocument()
    expect(within(queue).getByText('no imported test evidence provides `tests/x.rs`')).toBeInTheDocument()
  })

  test('opens on the first target of the walk, with its place and what remains', () => {
    renderAt('/review', queued())

    expect(screen.getByRole('heading', { name: 'structure:leaf' })).toBeInTheDocument()
    expect(screen.getByText('Target 1 of 3 · 2 after this one')).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Previous' })).toBeNull()
  })

  test('skipping moves to the next target of the walk, in queue order', async () => {
    const user = userEvent.setup()
    renderAt('/review', queued())

    await user.click(screen.getByRole('link', { name: 'Skip' }))
    expect(screen.getByRole('heading', { name: 'structure:other' })).toBeInTheDocument()

    await user.click(screen.getByRole('link', { name: 'Skip' }))
    expect(screen.getByRole('heading', { name: 'objective:root' })).toBeInTheDocument()
    expect(screen.getByText('Target 3 of 3 · 0 after this one')).toBeInTheDocument()
    expect(screen.queryByRole('link', { name: 'Skip' })).toBeNull()
  })

  test('opens any queued target directly: the order is not a gate', async () => {
    const user = userEvent.setup()
    renderAt('/review', queued())

    await user.click(screen.getByRole('link', { name: /structure:vetoed/ }))

    expect(screen.getByRole('heading', { name: 'structure:vetoed' })).toBeInTheDocument()
    expect(screen.getByText('Not in the walk · 3 targets can take a decision')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /structure:vetoed/ })).toHaveAttribute('aria-current', 'true')
  })

  test('follows a child to that child’s own local view', async () => {
    const user = userEvent.setup()
    renderAt('/review/root', queued())

    await user.click(within(screen.getByRole('region', { name: 'Contract content' })).getByRole('link', { name: 'other' }))

    expect(screen.getByRole('heading', { name: 'structure:other' })).toBeInTheDocument()
  })

  test('says so when asked for a target the argument does not have', () => {
    renderAt('/review/ghost', queued())

    expect(screen.getByRole('alert')).toHaveTextContent('no block named ghost')
  })

  test('says so when nothing needs a decision', () => {
    const page = queued()
    page.queue = { walk: [], groups: [] }
    renderAt('/review', page)

    expect(screen.getByText('nothing needs review')).toBeInTheDocument()
    expect(screen.getByText('Nothing needs a decision.')).toBeInTheDocument()
  })

  test('shows the aocs queue exactly as the server sent it', () => {
    const page = aocsPage as unknown as Snapshot
    renderAt('/review', page)

    const queue = screen.getByRole('navigation', { name: 'Review queue' })
    const listed = within(queue)
      .getAllByRole('link')
      .map((link) => link.querySelector('.queue__id')?.textContent)
    expect(listed).toEqual(page.queue.groups.flatMap((group) => group.targets))
    expect(screen.getByRole('heading', { level: 2 })).toHaveTextContent(page.queue.walk[0] as string)
  })
})
