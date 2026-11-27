/**
 * The checked-in pages are what `inka serve` sends for the two example
 * projects (`cargo test -p inka-core` holds them to that). Here they are held
 * to what the surfaces rely on, so a change on the Rust side that they cannot
 * draw fails this suite rather than the browser.
 */

import { describe, expect, test } from 'vitest'

import aocsPage from '../../fixtures/aocs.page.json'
import dogfoodPage from '../../fixtures/dogfood.page.json'
import type { Page } from '../generated/Page'
import type { Snapshot } from '../generated/Snapshot'
import { graphRoots, visibleNodes, allExpanded } from '../graph/visibility'
import { nodeOf, viewOf } from './view'

/** A fixture as the page it is; JSON modules lose the literal types. */
function snapshotOf(page: unknown): Snapshot {
  const typed = page as Page
  if (typed.kind !== 'argument') throw new Error('the fixture projects check')
  return typed
}

describe.each([
  ['aocs', aocsPage],
  ['dogfood', dogfoodPage],
])('the %s page', (_name, page) => {
  const snapshot = snapshotOf(page)
  const view = viewOf(snapshot)

  test('yields one entry per node', () => {
    expect(view.nodes.size).toBeGreaterThan(0)
    expect(view.order).toHaveLength(view.nodes.size)
    expect(snapshot.total).toBe(view.nodes.size)
  })

  test('hangs from exactly one objective, with no node unreachable', () => {
    expect(snapshot.roots).toHaveLength(1)
    expect(snapshot.unreachable_roots).toEqual([])
  })

  test('places every parent before its children', () => {
    const rank = new Map(view.order.map((id, index) => [id, index]))

    for (const node of view.nodes.values()) {
      for (const child of node.children) {
        expect(rank.get(node.id)).toBeLessThan(rank.get(child) as number)
      }
    }
  })

  test('names only nodes it defines', () => {
    const named = [
      ...snapshot.roots,
      ...snapshot.queue.walk,
      ...snapshot.queue.groups.flatMap((group) => group.targets),
      ...snapshot.nodes.flatMap((node) => [...node.parents, ...node.children]),
    ]

    for (const id of named) expect(view.nodes.has(id)).toBe(true)
  })

  test('lists a node’s children in its local view exactly as its relations do', () => {
    for (const node of view.nodes.values()) {
      expect((node.local.children?.entries ?? []).map((entry) => entry.id)).toEqual(node.children)
    }
  })

  test('shows every node once the whole graph is expanded', () => {
    const shown = visibleNodes(view.nodes, graphRoots(snapshot), allExpanded(view.nodes))

    expect([...shown].sort()).toEqual([...view.nodes.keys()].sort())
  })
})

describe('the aocs page specifically', () => {
  const snapshot = snapshotOf(aocsPage)
  const view = viewOf(snapshot)

  test('is the 17-node argument', () => {
    expect(view.nodes.size).toBe(17)
    expect(snapshot.roots).toEqual(['aocs_ok'])
  })

  test('shows a shared node once, with both its parents', () => {
    const shared = nodeOf(view, 'actuator_authority_ok')

    expect(shared.parents).toEqual(['momentum_ok', 'pointing_ok'])
    expect(view.order.filter((id) => id === shared.id)).toHaveLength(1)
  })

  test('carries the evidence target and the deadline the argument declares', () => {
    expect(nodeOf(view, 'detumble_convergence_ok').local.evidence).toMatchObject({
      kind: 'veriq',
      target: 'aocs.verify_detumble',
    })
    expect(nodeOf(view, 'mass_properties_valid').local.deadline).toMatchObject({
      until: 'FM',
      promote: { mode: 'bind' },
    })
  })

  test('interpolates every value, so no `{$var}` reaches the page', () => {
    for (const node of view.nodes.values()) {
      expect(node.local.description).not.toMatch(/\{\$/)
    }
  })
})
