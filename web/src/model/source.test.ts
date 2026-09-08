import { afterEach, describe, expect, test, vi } from 'vitest'

import { edge, node } from '../test/factory'
import { fetchElements, INJECTED_ELEMENT_ID, loadView, readInjected, SourceError } from './source'

function inject(text: string): void {
  const script = document.createElement('script')
  script.type = 'application/json'
  script.id = INJECTED_ELEMENT_ID
  script.textContent = text
  document.body.append(script)
}

afterEach(() => {
  document.getElementById(INJECTED_ELEMENT_ID)?.remove()
  vi.unstubAllGlobals()
})

describe('readInjected', () => {
  test('returns null when the page carries no payload', () => {
    expect(readInjected()).toBeNull()
  })

  test('reads a payload injected by the server', () => {
    inject(JSON.stringify([node('a')]))

    expect(readInjected()).toEqual([node('a')])
  })

  test('reports an injected payload that is not JSON', () => {
    inject('{ nope')

    expect(() => readInjected()).toThrow(SourceError)
  })
})

describe('fetchElements', () => {
  test('returns the parsed body of a successful response', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json([node('a')])))

    await expect(fetchElements('/elements.json')).resolves.toEqual([node('a')])
  })

  test('reports the status of a failed response', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 404 })))

    await expect(fetchElements('/elements.json')).rejects.toThrow(/404/)
  })

  test('reports a response that is not JSON', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('<html>', { status: 200 })))

    await expect(fetchElements('/elements.json')).rejects.toThrow(/did not answer JSON/)
  })

  test('reports an unreachable endpoint', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')))

    await expect(fetchElements('/elements.json')).rejects.toThrow(/could not reach/)
  })
})

describe('loadView', () => {
  test('prefers the injected payload over the endpoint', async () => {
    inject(JSON.stringify([node('a'), node('b'), edge('a', 'b')]))
    const fetcher = vi.fn()
    vi.stubGlobal('fetch', fetcher)

    const view = await loadView('/elements.json')

    expect(view.order).toEqual(['a', 'b'])
    expect(fetcher).not.toHaveBeenCalled()
  })

  test('falls back to the endpoint when nothing is injected', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json([node('a')])))

    await expect(loadView('/elements.json')).resolves.toMatchObject({ order: ['a'] })
  })

  test('surfaces a malformed export rather than rendering half an argument', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json([{ data: { id: 'a' } }])))

    await expect(loadView('/elements.json')).rejects.toThrow(/malformed inka export/)
  })
})
