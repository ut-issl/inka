/**
 * The checked-in pages are what `inka serve` sends for the two example
 * projects (`cargo test -p inka-core` holds them to that). Here they are held
 * to what the viewer relies on, so a change on the Rust side that the viewer
 * cannot draw fails this suite rather than the browser.
 */

import { describe, expect, test } from 'vitest'

import aocsPage from '../../fixtures/aocs.page.json'
import dogfoodPage from '../../fixtures/dogfood.page.json'
import type { Page } from '../generated/Page'
import type { Snapshot } from '../generated/Snapshot'
import { namespacesOf, nodeOf, rootsOf, viewOf } from './view'

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

  test('has exactly one objective root', () => {
    expect(rootsOf(view)).toHaveLength(1)
  })

  test('lists every node exactly once across the namespace groups', () => {
    const listed = view.groups.flatMap((group) => group.nodes)

    expect([...listed].sort()).toEqual([...view.order].sort())
  })

  test('places every parent before its children', () => {
    const rank = new Map(view.order.map((id, index) => [id, index]))

    for (const edge of view.edges) {
      expect(rank.get(edge.source)).toBeLessThan(rank.get(edge.target) as number)
    }
  })

  test('names only nodes it defines', () => {
    for (const edge of view.edges) {
      expect(view.nodes.has(edge.source) && view.nodes.has(edge.target)).toBe(true)
    }
    for (const id of [...snapshot.queue.walk, ...snapshot.queue.groups.flatMap((group) => group.targets)]) {
      expect(view.nodes.has(id)).toBe(true)
    }
  })
})

describe('the aocs page specifically', () => {
  const view = viewOf(snapshotOf(aocsPage))

  test('is the 17-node argument', () => {
    expect(view.nodes.size).toBe(17)
    expect(rootsOf(view).map((root) => root.id)).toEqual(['aocs_ok'])
  })

  test('groups the namespaces parents-first', () => {
    expect(namespacesOf(view)).toEqual(['objective', 'safety', 'momentum', 'pointing', 'assumptions'])
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

  test('interpolates every value, so no `{$var}` reaches the viewer', () => {
    for (const node of view.nodes.values()) {
      expect(node.local.description).not.toMatch(/\{\$/)
    }
  })
})
