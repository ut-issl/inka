import { afterEach, describe, expect, test, vi } from 'vitest'

import { loadSession, recordDecision } from './session'

const reviewer = { approver: 'iwata', key_id: 'iwata', token: 'the-token' }

afterEach(() => vi.unstubAllGlobals())

describe('loadSession', () => {
  test('returns who the server records as', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ reviewer })))

    await expect(loadSession('./api/session')).resolves.toEqual({ reviewer })
  })

  test('treats a server that cannot be asked as one that does not record', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')))
    await expect(loadSession('./api/session')).resolves.toEqual({ reviewer: null })

    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('nope', { status: 404 })))
    await expect(loadSession('./api/session')).resolves.toEqual({ reviewer: null })

    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ kind: 'argument' })))
    await expect(loadSession('./api/session')).resolves.toEqual({ reviewer: null })
  })
})

describe('recordDecision', () => {
  test('posts the target, the hash on screen and the decision, under the session token', async () => {
    const fetcher = vi.fn().mockResolvedValue(Response.json({ outcome: 'recorded' }))
    vi.stubGlobal('fetch', fetcher)

    const outcome = await recordDecision('./api/decisions', reviewer, 'leaf', 'sha256:ab', {
      label: 'decline',
      comment: 'not yet',
    })

    expect(outcome).toEqual({ outcome: 'recorded' })
    const [url, init] = fetcher.mock.calls[0] as [string, RequestInit]
    expect(url).toBe('./api/decisions')
    expect(init.method).toBe('POST')
    expect(init.headers).toMatchObject({ 'Content-Type': 'application/json', 'X-Inka-Session': 'the-token' })
    expect(JSON.parse(init.body as string)).toEqual({
      target: 'leaf',
      view_hash: 'sha256:ab',
      decision: { label: 'decline', comment: 'not yet' },
    })
  })

  test('sends nothing that says who is deciding: that is the server’s', async () => {
    const fetcher = vi.fn().mockResolvedValue(Response.json({ outcome: 'recorded' }))
    vi.stubGlobal('fetch', fetcher)

    await recordDecision('./api/decisions', reviewer, 'leaf', 'sha256:ab', { label: 'approve', comment: null })

    const body = (fetcher.mock.calls[0] as [string, RequestInit])[1].body as string
    expect(body).not.toContain('iwata')
  })

  test('passes on what the server says when it does not record', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ outcome: 'changed' }, { status: 409 })))

    await expect(
      recordDecision('./api/decisions', reviewer, 'leaf', 'sha256:ab', { label: 'approve', comment: null }),
    ).resolves.toEqual({ outcome: 'changed' })
  })

  test('reports a request that was turned away or never arrived as a failure with the reason', async () => {
    const decision = { label: 'approve', comment: null } as const
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('not from this server', { status: 403 })))
    await expect(recordDecision('./api/decisions', reviewer, 'leaf', 'sha256:ab', decision)).resolves.toEqual({
      outcome: 'failed',
      message: 'not from this server',
    })

    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('offline')))
    await expect(recordDecision('./api/decisions', reviewer, 'leaf', 'sha256:ab', decision)).resolves.toMatchObject({
      outcome: 'failed',
      message: expect.stringContaining('offline'),
    })
  })
})
