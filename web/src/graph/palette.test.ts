import { describe, expect, test } from 'vitest'

import { colorOf, namespaceColors } from './palette'

describe('namespaceColors', () => {
  test('gives each namespace one colour, stable for a given order', () => {
    const colors = namespaceColors(['a', 'b', 'c'])

    expect(colorOf(colors, 'a')).toBe(colorOf(namespaceColors(['a', 'b', 'c']), 'a'))
    expect(colorOf(colors, 'a')).not.toBe(colorOf(colors, 'b'))
  })

  test('cycles the palette rather than running out of colours', () => {
    const many = Array.from({ length: 20 }, (_, index) => `ns${index}`)

    const colors = namespaceColors(many)

    expect(colors.size).toBe(20)
    expect(colorOf(colors, 'ns0')).toBe(colorOf(colors, 'ns8'))
  })

  test('falls back to a neutral colour for an unknown namespace', () => {
    expect(colorOf(namespaceColors(['a']), 'ghost')).toBe('#94a3b8')
  })
})
