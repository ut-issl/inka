import { describe, expect, test } from 'vitest'

import { DISCHARGE_MODES } from '../model/elements'
import { MODE_COLORS, stylesheet } from './stylesheet'

type Block = { selector: string; style: Record<string, unknown> }

const blocks = stylesheet() as Block[]
const find = (selector: string) => blocks.find((block) => block.selector === selector) as Block

describe('stylesheet', () => {
  test('labels a node with its id, never with anything else', () => {
    expect(find('node').style['label']).toBe('data(id)')
  })

  test('auto-sizes a node to its label', () => {
    expect(find('node').style['width']).toBe('label')
    expect(find('node').style['height']).toBe('label')
  })

  test('gives each of the four discharge modes a distinct shape-and-fill pair', () => {
    const looks = DISCHARGE_MODES.map((mode) => {
      const style = find(`node[mode = "${mode}"]`).style
      return `${String(style['shape'])}/${String(style['background-color'])}`
    })

    expect(new Set(looks).size).toBe(DISCHARGE_MODES.length)
  })

  test('draws every edge with the same weight and the arrowhead at the child', () => {
    const style = find('edge').style

    expect(style['width']).toBe(2)
    expect(style['target-arrow-shape']).toBe('triangle')
    expect(style['source-arrow-shape']).toBeUndefined()
  })

  test('offers a swatch for every mode, so the list and the legend agree with the graph', () => {
    for (const mode of DISCHARGE_MODES) {
      expect(MODE_COLORS[mode]).toMatch(/^#[0-9a-f]{6}$/i)
    }
  })
})
