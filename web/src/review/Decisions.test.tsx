import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, describe, expect, test, vi } from 'vitest'

import type { Snapshot } from '../generated/Snapshot'
import { viewOf } from '../model/view'
import { edge, node, snapshot } from '../test/factory'
import { ReviewSurface } from './ReviewSurface'

const reviewer = { approver: 'iwata', key_id: 'iwata', token: 'the-token' }

/** Two targets to walk; a third is declined and takes no decision. */
function queued(): Snapshot {
  const page = snapshot([
    node('root', { mode: 'objective', root: true }),
    node('leaf'),
    node('vetoed'),
    edge('root', 'leaf'),
    edge('root', 'vetoed'),
  ])
  page.nodes.find((candidate) => candidate.id === 'vetoed')!.review = { state: 'declined', by: ['sato'] }
  page.queue = {
    walk: ['leaf', 'root'],
    groups: [
      { state: 'unapproved', targets: ['leaf', 'root'] },
      { state: 'declined', targets: ['vetoed'] },
    ],
  }
  return page
}

function renderAt(path: string, options: { reviewer?: typeof reviewer | null; onReload?: () => void } = {}) {
  const page = queued()
  render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route
          path="/review/:id?"
          element={
            <ReviewSurface
              snapshot={page}
              nodes={viewOf(page).nodes}
              reviewer={options.reviewer === undefined ? reviewer : options.reviewer}
              {...(options.onReload ? { onReload: options.onReload } : {})}
            />
          }
        />
      </Routes>
    </MemoryRouter>,
  )
}

function answer(body: unknown, status = 200) {
  const fetcher = vi.fn().mockResolvedValue(Response.json(body, { status }))
  vi.stubGlobal('fetch', fetcher)
  return fetcher
}

function sent(fetcher: ReturnType<typeof vi.fn>) {
  return JSON.parse((fetcher.mock.calls[0] as [string, RequestInit])[1].body as string) as unknown
}

afterEach(() => vi.unstubAllGlobals())

describe('recording a decision', () => {
  test('a read-only server offers no decision anywhere', () => {
    renderAt('/review/leaf', { reviewer: null })

    expect(screen.queryByRole('form', { name: 'Decision' })).toBeNull()
    expect(screen.queryByRole('button', { name: /approve|decline/i })).toBeNull()
  })

  test('a recording server offers one under the local view, saying who it records as', () => {
    renderAt('/review/leaf')

    const form = screen.getByRole('form', { name: 'Decision' })
    expect(form).toHaveTextContent('Recording as iwata')
    // The controls come after the whole local view, not before or inside it.
    const view = screen.getByRole('article')
    expect(view.compareDocumentPosition(form) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  test('a target that cannot take a decision gets no controls', () => {
    renderAt('/review/vetoed')

    expect(screen.getByRole('heading', { name: 'structure:vetoed' })).toBeInTheDocument()
    expect(screen.queryByRole('form', { name: 'Decision' })).toBeNull()
  })

  test('approving records against the hash on screen and moves on to the next target', async () => {
    const user = userEvent.setup()
    const fetcher = answer({ outcome: 'recorded' })
    const onReload = vi.fn()
    renderAt('/review/leaf', { onReload })

    await user.click(screen.getByRole('button', { name: 'Approve' }))

    expect(sent(fetcher)).toEqual({
      target: 'leaf',
      view_hash: `sha256:${'0'.repeat(64)}`,
      decision: { label: 'approve', comment: null },
    })
    expect(onReload).toHaveBeenCalledOnce()
    expect(await screen.findByRole('heading', { name: 'objective:root' })).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Recorded approve of structure:leaf.')
  })

  test('an approval may carry a comment', async () => {
    const user = userEvent.setup()
    const fetcher = answer({ outcome: 'recorded' })
    renderAt('/review/leaf')

    await user.type(screen.getByLabelText('Comment'), '  checked against the budget ')
    await user.click(screen.getByRole('button', { name: 'Approve' }))

    expect(sent(fetcher)).toMatchObject({ decision: { label: 'approve', comment: 'checked against the budget' } })
  })

  test('a decline needs a reason before it can be sent', async () => {
    const user = userEvent.setup()
    const fetcher = answer({ outcome: 'recorded' })
    renderAt('/review/leaf')
    expect(screen.getByRole('button', { name: 'Decline' })).toBeDisabled()

    await user.type(screen.getByLabelText('Comment'), '   ')
    expect(screen.getByRole('button', { name: 'Decline' })).toBeDisabled()

    await user.type(screen.getByLabelText('Comment'), 'the margin is not shown')
    await user.click(screen.getByRole('button', { name: 'Decline' }))

    expect(sent(fetcher)).toMatchObject({ decision: { label: 'decline', comment: 'the margin is not shown' } })
  })

  test('content that changed since it was shown is not recorded: the page asks again and says so', async () => {
    const user = userEvent.setup()
    answer({ outcome: 'changed' }, 409)
    const onReload = vi.fn()
    renderAt('/review/leaf', { onReload })

    await user.click(screen.getByRole('button', { name: 'Approve' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('has changed since it was shown')
    expect(onReload).toHaveBeenCalledOnce()
    // Still on the same target, to read what it says now.
    expect(screen.getByRole('heading', { name: 'structure:leaf' })).toBeInTheDocument()
  })

  test('a refusal is shown with its reason and nothing moves', async () => {
    const user = userEvent.setup()
    answer({ outcome: 'refused', message: 'no block named `leaf` in the argument' }, 422)
    const onReload = vi.fn()
    renderAt('/review/leaf', { onReload })

    await user.click(screen.getByRole('button', { name: 'Approve' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('Nothing was recorded: no block named `leaf`')
    expect(onReload).not.toHaveBeenCalled()
    expect(screen.getByRole('heading', { name: 'structure:leaf' })).toBeInTheDocument()
  })
})
