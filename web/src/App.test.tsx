import { render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { edge, node, snapshot } from './test/factory'
import { App } from './App'

afterEach(() => vi.unstubAllGlobals())

describe('App', () => {
  test('says so while the argument is loading', () => {
    vi.stubGlobal('fetch', vi.fn(() => new Promise(() => {})))

    render(<App />)

    expect(screen.getByText(/Loading the argument/)).toBeInTheDocument()
  })

  test('shows why the argument could not be loaded instead of an empty graph', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')))

    render(<App />)

    await waitFor(() => expect(screen.getByRole('alert')).toBeInTheDocument())
    expect(screen.getByText(/could not reach/i)).toBeInTheDocument()
  })

  test('opens on the review surface, with both surfaces and the count in sight', async () => {
    const page = { kind: 'argument', ...snapshot([node('a', { mode: 'objective', root: true }), node('b'), edge('a', 'b')]) }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(page)))

    render(<App />)

    await waitFor(() => expect(screen.getByRole('navigation', { name: 'Review queue' })).toBeInTheDocument())
    expect(screen.getByRole('link', { name: 'Review' })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Graph' })).toBeInTheDocument()
    expect(screen.getByText('established 0/2')).toBeInTheDocument()
  })

  test('shows the findings of a project that does not check, each with where it points', async () => {
    const findings = [
      {
        severity: 'error',
        message: 'expected a description fence',
        location: { path: 'argument/a.inka', line: 3, column: 5 },
      },
      { severity: 'error', message: 'no objective block found', location: null },
    ]
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ kind: 'rejected', findings })))

    render(<App />)

    await waitFor(() => expect(screen.getByRole('alert')).toBeInTheDocument())
    expect(screen.getByText('argument/a.inka:3:5')).toBeInTheDocument()
    expect(screen.getByText('expected a description fence')).toBeInTheDocument()
    expect(screen.getByText('no objective block found')).toBeInTheDocument()
  })
})
