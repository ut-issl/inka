/**
 * The checked-in fixture exports are the contract's worked examples: they must
 * validate, and the orderings the UI shows must come out of them unaided.
 */

import { describe, expect, test } from 'vitest'

import aocs from '../../fixtures/aocs.elements.json'
import dogfood from '../../fixtures/dogfood.elements.json'
import { parseElements } from './elements'
import { buildView, namespacesOf, nodeOf, rootsOf } from './view'

describe.each([
  ['aocs', aocs],
  ['dogfood', dogfood],
])('the %s export', (_name, elements) => {
  const view = buildView(parseElements(elements))

  test('validates and yields one entry per node', () => {
    expect(view.nodes.size).toBeGreaterThan(0)
    expect(view.order).toHaveLength(view.nodes.size)
  })

  test('has exactly one objective root', () => {
    expect(rootsOf(view)).toHaveLength(1)
  })

  test('lists every node exactly once across the namespace groups', () => {
    const listed = view.groups.flatMap((group) => group.nodeIds)

    expect([...listed].sort()).toEqual([...view.order].sort())
  })

  test('places every parent before its children', () => {
    const rank = new Map(view.order.map((id, index) => [id, index]))

    for (const edge of view.edges) {
      expect(rank.get(edge.source)).toBeLessThan(rank.get(edge.target) as number)
    }
  })
})

describe('the aocs export specifically', () => {
  const view = buildView(parseElements(aocs))

  test('is the 17-node argument the design page shows', () => {
    expect(view.nodes.size).toBe(17)
    expect(rootsOf(view).map((root) => root.id)).toEqual(['aocs_ok'])
  })

  test('groups the namespaces in the order the design page shows', () => {
    expect(namespacesOf(view)).toEqual(['objective', 'safety', 'momentum', 'pointing', 'assumptions'])
  })

  test('shows a shared node once, with both its parents', () => {
    const shared = nodeOf(view, 'actuator_authority_ok')

    expect(shared.parents).toEqual(['momentum_ok', 'pointing_ok'])
    expect(view.order.filter((id) => id === shared.id)).toHaveLength(1)
  })

  test('carries the evidence target and the deadline the argument declares', () => {
    expect(nodeOf(view, 'detumble_convergence_ok').evidence).toEqual({
      kind: 'veriq',
      target: 'aocs.verify_detumble',
    })
    expect(nodeOf(view, 'mass_properties_valid').deadline).toMatchObject({
      until: 'FM',
      promote: 'bind',
    })
  })

  test('interpolates every value, so no `{$var}` reaches the viewer', () => {
    for (const node of view.nodes.values()) {
      expect(node.description).not.toMatch(/\{\$/)
    }
  })
})
