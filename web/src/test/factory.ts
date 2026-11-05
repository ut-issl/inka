/** Builders for the small arguments the model tests reason about. */

import type { ArgumentElement } from '../model/elements'

type NodeOverrides = Partial<{
  mode: 'objective' | 'structure' | 'bind' | 'fiat'
  namespace: string
  description: string
  tags: string[]
  root: boolean
  evidence: { kind: 'veriq' | 'test'; target: string }
  deadline: { until: string; promote?: 'structure' | 'bind' }
  comments: Partial<Record<'block' | 'children' | 'test' | 'veriq' | 'until' | 'promote', string>>
}>

export function node(id: string, overrides: NodeOverrides = {}): ArgumentElement {
  return {
    data: {
      id,
      mode: 'structure',
      namespace: 'root',
      description: `${id} description`,
      tags: [],
      root: false,
      comments: {},
      ...overrides,
    },
  }
}

export function edge(source: string, target: string, comment?: string): ArgumentElement {
  const data = { id: `${source}->${target}`, source, target }
  return { data: comment === undefined ? data : { ...data, comment } }
}
