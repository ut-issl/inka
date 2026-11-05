import { describe, expect, test } from 'vitest'

import { ElementsError, isEdgeData, parseElements } from './elements'
import { edge, node } from '../test/factory'

describe('parseElements', () => {
  test('accepts a node element and defaults its optional fields', () => {
    // Arrange
    const payload = [{ data: { id: 'a', mode: 'fiat', namespace: 'ns', description: 'text' } }]

    // Act
    const [element] = parseElements(payload)

    // Assert
    expect(element?.data).toMatchObject({ id: 'a', mode: 'fiat', tags: [], root: false })
  })

  test('keeps the evidence target and the deadline of a bind node', () => {
    const payload = [
      node('a', {
        mode: 'bind',
        evidence: { kind: 'veriq', target: 'pkg.verify' },
        deadline: { until: 'FM', promote: 'bind' },
      }),
    ]

    const [element] = parseElements(payload)

    expect(element?.data).toMatchObject({
      evidence: { kind: 'veriq', target: 'pkg.verify' },
      deadline: { until: 'FM', promote: 'bind' },
    })
  })

  test('distinguishes edge payloads from node payloads', () => {
    const parsed = parseElements([node('a'), edge('a', 'b')])

    expect(parsed.map((element) => isEdgeData(element.data))).toEqual([false, true])
  })

  test('rejects an unknown discharge mode', () => {
    const payload = [{ data: { id: 'a', mode: 'guess', namespace: 'ns', description: '' } }]

    expect(() => parseElements(payload)).toThrow(ElementsError)
  })

  test('rejects a payload that is not an array', () => {
    expect(() => parseElements({ elements: [] })).toThrow(ElementsError)
  })

  test('reports which field was wrong', () => {
    expect(() => parseElements([{ data: { id: 'a', mode: 'fiat', description: 'x' } }])).toThrow(
      /namespace/,
    )
  })
})
