import { render, screen, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { describe, expect, test, vi } from 'vitest'

import { viewOf } from '../model/view'
import { edge, node, snapshot } from '../test/factory'
import { GraphSurface } from './GraphSurface'

// The canvas measures its cards, which only a browser does; `graph.spec.ts`
// covers it there. Here it is stood in for, and what surrounds it is tested.
vi.mock('./CardGraph', () => ({
  CardGraph: ({ visible }: { visible: readonly string[] }) => <div data-testid="canvas">{visible.join(' ')}</div>,
}))

function page() {
  const built = snapshot([
    node('root', { mode: 'objective', root: true }),
    node('branch'),
    node('leaf', { mode: 'fiat' }),
    edge('root', 'branch'),
    edge('branch', 'leaf'),
  ])
  // `leaf` is settled: established, and in no queue.
  const leaf = built.nodes.find((candidate) => candidate.id === 'leaf')!
  leaf.status = { state: 'established' }
  leaf.review = null
  return built
}

function renderAt(path: string) {
  const built = page()
  render(
    <MemoryRouter initialEntries={[path]}>
      <Routes>
        <Route path="/graph/:id?" element={<GraphSurface snapshot={built} nodes={viewOf(built).nodes} />} />
      </Routes>
    </MemoryRouter>,
  )
}

describe('GraphSurface', () => {
  test('opens with the roots and their direct children', () => {
    renderAt('/graph')

    expect(screen.getByTestId('canvas')).toHaveTextContent('root branch')
    expect(screen.getByText('2 of 3 nodes shown')).toBeInTheDocument()
    expect(screen.queryByRole('region', { name: 'Selected node' })).toBeNull()
  })

  test('a node opened from elsewhere is revealed, with its local view beside the graph', () => {
    renderAt('/graph/leaf')

    expect(screen.getByTestId('canvas')).toHaveTextContent('root branch leaf')
    const panel = screen.getByRole('region', { name: 'Selected node' })
    expect(within(panel).getByRole('heading', { level: 2 })).toHaveTextContent('fiat:leaf')
  })

  test('leads to the same node on the review surface, whether or not it is queued', () => {
    renderAt('/graph/leaf')
    const settled = screen.getByRole('region', { name: 'Selected node' })
    expect(within(settled).getByRole('link', { name: 'Open in review' })).toHaveAttribute('href', '/review/leaf')
  })

  test('offers no decision on the selected node', () => {
    renderAt('/graph/branch')

    const panel = screen.getByRole('region', { name: 'Selected node' })
    expect(within(panel).queryByRole('button', { name: /approve|decline/i })).toBeNull()
    expect(within(panel).queryByRole('form')).toBeNull()
  })
})
