/**
 * Builders for the small arguments the tests reason about.
 *
 * A test lists nodes and entries; {@link snapshot} assembles the page the
 * server would send for them. What the server works out — parents and the
 * order — is filled in here in the simplest way that is right
 * for a list written parents-first, which is how the tests write theirs: the
 * order is the order of the list. The real derivations are Rust's and are
 * tested there; `fixtures/*.page.json` are their output.
 */

import type { NodeData } from '../generated/NodeData'
import type { Snapshot } from '../generated/Snapshot'
import { viewOf, type ArgumentView } from '../model/view'

type Mode = NodeData['mode']

/** Comments by the anchor they are attached to, as the DSL names them. */
interface CommentSpec {
  block?: string
  children?: string
  test?: string
  veriq?: string
  until?: string
  promote?: string
}

interface NodeSpec {
  readonly kind: 'node'
  readonly id: string
  readonly mode: Mode
  readonly namespace: string
  readonly description: string
  readonly tags: string[]
  readonly root: boolean
  readonly evidence?: { kind: 'veriq' | 'test'; target: string }
  readonly deadline?: { until: string; promote?: Mode }
  readonly comments: CommentSpec
}

interface EdgeSpec {
  readonly kind: 'edge'
  readonly source: string
  readonly target: string
  readonly comment: string | null
}

export type Spec = NodeSpec | EdgeSpec

type NodeOverrides = Partial<Omit<NodeSpec, 'kind' | 'id'>>

export function node(id: string, overrides: NodeOverrides = {}): Spec {
  return {
    kind: 'node',
    id,
    mode: 'structure',
    namespace: 'root',
    description: `${id} description`,
    tags: [],
    root: false,
    comments: {},
    ...overrides,
  }
}

export function edge(source: string, target: string, comment?: string): Spec {
  return { kind: 'edge', source, target, comment: comment ?? null }
}

/** The page for the listed nodes and entries; nodes are listed parents-first. */
export function snapshot(specs: readonly Spec[]): Snapshot {
  const nodeSpecs = specs.filter((spec) => spec.kind === 'node')
  const edgeSpecs = specs.filter((spec) => spec.kind === 'edge')
  const byId = new Map(nodeSpecs.map((spec) => [spec.id, spec]))
  const sorted = [...byId.keys()].sort()

  const nodes: NodeData[] = sorted.map((id) => {
    const spec = byId.get(id) as NodeSpec
    const entries = edgeSpecs.filter((entry) => entry.source === id)
    const evidence = spec.evidence
    return {
      id,
      mode: spec.mode,
      namespace: spec.namespace,
      tags: spec.tags,
      root: spec.root,
      parents: sorted.filter((parent) =>
        edgeSpecs.some((entry) => entry.source === parent && entry.target === id),
      ),
      children: entries.map((entry) => entry.target),
      below: 0,
      blamed_below: 0,
      status: { state: 'blamed', reasons: [{ cause: 'missing_approval' }] },
      review: { state: 'unapproved' },
      view_hash: `sha256:${'0'.repeat(64)}`,
      local: {
        id,
        mode: spec.mode,
        comment: spec.comments.block ?? null,
        description: spec.description,
        evidence: evidence
          ? { ...evidence, comment: spec.comments[evidence.kind] ?? null }
          : null,
        children:
          entries.length > 0
            ? {
                comment: spec.comments.children ?? null,
                entries: entries.map((entry) => ({
                  id: entry.target,
                  comment: entry.comment,
                  description: byId.get(entry.target)?.description ?? null,
                })),
              }
            : null,
        deadline: spec.deadline
          ? {
              until: spec.deadline.until,
              until_comment: spec.comments.until ?? null,
              promote: spec.deadline.promote
                ? { mode: spec.deadline.promote, comment: spec.comments.promote ?? null }
                : null,
            }
          : null,
      },
    }
  })

  const order = nodeSpecs.map((spec) => spec.id)
  return {
    established: 0,
    total: nodes.length,
    nodes,
    roots: sorted.filter((id) => byId.get(id)?.root),
    unreachable_roots: sorted.filter(
      (id) => !byId.get(id)?.root && !edgeSpecs.some((entry) => entry.target === id),
    ),
    order,
    queue: { walk: [], groups: [] },
    warnings: [],
  }
}

/** The view over {@link snapshot}. */
export function view(specs: readonly Spec[]): ArgumentView {
  return viewOf(snapshot(specs))
}
