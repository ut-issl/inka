import { render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'

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
})
