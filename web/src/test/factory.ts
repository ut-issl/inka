/** Builders for the small arguments the model tests reason about. */

import type { ArgumentElement } from '../model/elements'

type NodeOverrides = Partial<{
  mode: 'objective' | 'structure' | 'bind' | 'fiat'
  namespace: string
  description: string
  tags: string[]
  root: boolean
  evidence: { kind: 'veriq' | 'ci'; target: string }
  deadline: { until: string; reason?: string; promote?: 'structure' | 'bind' }
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
      ...overrides,
    },
  }
}

export function edge(source: string, target: string): ArgumentElement {
  return { data: { id: `${source}->${target}`, source, target } }
}
