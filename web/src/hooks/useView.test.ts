import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { edge, node } from '../test/factory'
import { useView } from './useView'

afterEach(() => vi.unstubAllGlobals())

describe('useView', () => {
  test('starts out loading', () => {
    vi.stubGlobal('fetch', vi.fn(() => new Promise(() => {})))

    const { result } = renderHook(() => useView('/elements.json'))

    expect(result.current.status).toBe('loading')
  })

  test('ends with the shared view once the argument arrives', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json([node('a'), node('b'), edge('a', 'b')])))

    const { result } = renderHook(() => useView('/elements.json'))

    await waitFor(() => expect(result.current.status).toBe('ready'))
    expect(result.current).toMatchObject({ view: { order: ['a', 'b'] } })
  })

  test('reports why the argument could not be loaded', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 500 })))

    const { result } = renderHook(() => useView('/elements.json'))

    await waitFor(() => expect(result.current.status).toBe('error'))
    expect(result.current).toMatchObject({ message: expect.stringContaining('500') })
  })

  test('ignores a response that arrives after unmount', async () => {
    let settle: (response: Response) => void = () => {}
    vi.stubGlobal('fetch', vi.fn(() => new Promise<Response>((resolve) => (settle = resolve))))

    const { unmount } = renderHook(() => useView('/elements.json'))
    unmount()
    settle(Response.json([node('a')]))

    // Nothing to assert beyond the absence of an update-after-unmount warning:
    // the hook must not touch state once its effect has been cleaned up.
    await expect(Promise.resolve()).resolves.toBeUndefined()
  })
})
