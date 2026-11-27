import { afterEach, describe, expect, test, vi } from 'vitest'

import { node, snapshot } from '../test/factory'
import { loadPage, SourceError } from './source'

afterEach(() => vi.unstubAllGlobals())

describe('loadPage', () => {
  test('returns the page of a successful response', async () => {
    const page = { kind: 'argument', ...snapshot([node('a')]) }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(page)))

    await expect(loadPage('./api/page')).resolves.toEqual(page)
  })

  test('returns a rejected project as its findings, not as a failure', async () => {
    const page = { kind: 'rejected', findings: [{ severity: 'error', message: 'broken', location: null }] }
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(page)))

    await expect(loadPage('./api/page')).resolves.toEqual(page)
  })

  test('reports the status of a failed response', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 404 })))

    await expect(loadPage('./api/page')).rejects.toThrow(/404/)
  })

  test('reports a response that is not JSON', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('<html>', { status: 200 })))

    await expect(loadPage('./api/page')).rejects.toThrow(/did not answer JSON/)
  })

  test('reports an unreachable server', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')))

    const failure = loadPage('./api/page')

    await expect(failure).rejects.toThrow(SourceError)
    await expect(failure).rejects.toThrow(/could not reach/)
  })
})
