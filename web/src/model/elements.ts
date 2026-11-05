/**
 * The `inka export` wire format: a cytoscape `elements` array.
 *
 * This module is the viewer's only boundary with inka. The viewer never parses
 * or mutates the argument — it validates this projection and reads it.
 *
 * A node element carries everything the UI shows about a block:
 *
 * ```json
 * { "data": { "id": "detumble_ok", "mode": "structure", "namespace": "safety",
 *             "description": "…", "tags": ["safety"], "root": false,
 *             "evidence": { "kind": "veriq", "target": "aocs.verify_detumble" },
 *             "deadline": { "until": "FM", "promote": "bind" },
 *             "comments": { "block": "…", "until": "…" } } }
 * ```
 *
 * An edge element points from a parent to one child — one `children:` entry —
 * and carries the comment on that entry, if any:
 *
 * ```json
 * { "data": { "id": "detumble_ok->mtq_authority_ok", "source": "detumble_ok",
 *             "target": "mtq_authority_ok", "comment": "…" } }
 * ```
 *
 * Two properties of the array are load-bearing and hold by construction on the
 * Rust side: node elements appear in sorted block-ID order (the view's own
 * order, which every deterministic ordering here derives from), and edge
 * elements appear in each parent's declaration order. There is no `label`
 * field: a node's label *is* its id, so the UI reads `data(id)` and the
 * invariant cannot drift.
 */

import { z } from 'zod'

/** The four discharge modes, in the order the UI presents them. */
export const DISCHARGE_MODES = ['objective', 'structure', 'bind', 'fiat'] as const

export type DischargeMode = (typeof DISCHARGE_MODES)[number]

/** Machine-verifiable evidence a bind block discharges against. */
const evidenceSchema = z.object({
  kind: z.enum(['veriq', 'test']),
  /** The reference verbatim, opaque to the viewer. */
  target: z.string().min(1),
})

/** An `until:` annotation, with the optional promotion target. */
const deadlineSchema = z.object({
  until: z.string().min(1),
  promote: z.enum(['structure', 'bind']).optional(),
})

/**
 * The comments attached to a block and to its properties, keyed by `block` or
 * the property name. Comments on `children:` entries ride on the edges.
 */
const commentsSchema = z.object({
  block: z.string().optional(),
  children: z.string().optional(),
  test: z.string().optional(),
  veriq: z.string().optional(),
  until: z.string().optional(),
  promote: z.string().optional(),
})

const nodeDataSchema = z.object({
  id: z.string().min(1),
  mode: z.enum(DISCHARGE_MODES),
  namespace: z.string().min(1),
  /** Interpolated contract text: no `{$var}` reaches the viewer. */
  description: z.string(),
  /** Free-form tags without the leading `#`. */
  tags: z.array(z.string()).default([]),
  /** Set on the argument's objective roots. */
  root: z.boolean().default(false),
  evidence: evidenceSchema.optional(),
  deadline: deadlineSchema.optional(),
  comments: commentsSchema.default({}),
})

const edgeDataSchema = z.object({
  id: z.string().min(1),
  source: z.string().min(1),
  target: z.string().min(1),
  /** The comment on the parent's `children:` entry for this child. */
  comment: z.string().optional(),
})

/** Shape shared by both kinds, checked before either is chosen. */
const envelopeSchema = z.array(z.object({ data: z.record(z.string(), z.unknown()) }))

const elementSchema = z.object({ data: z.union([edgeDataSchema, nodeDataSchema]) })

export type Evidence = z.infer<typeof evidenceSchema>
export type Deadline = z.infer<typeof deadlineSchema>
export type Comments = z.infer<typeof commentsSchema>
export type NodeData = z.infer<typeof nodeDataSchema>
export type EdgeData = z.infer<typeof edgeDataSchema>
export type ArgumentElement = z.infer<typeof elementSchema>

/** Raised when an export does not match the contract above. */
export class ElementsError extends Error {
  override readonly name = 'ElementsError'
}

/** Narrows an element's payload to an edge; anything else is a node. */
export function isEdgeData(data: NodeData | EdgeData): data is EdgeData {
  return 'source' in data
}

/**
 * Validates an `inka export` payload.
 *
 * Each element is checked against the schema its shape selects — an element
 * carrying `source` is an edge — rather than against a union, so a bad export
 * says which field of which element is wrong instead of "no alternative
 * matched".
 *
 * @throws {ElementsError} when the payload is not a well-formed elements array.
 */
export function parseElements(input: unknown): ArgumentElement[] {
  const envelope = envelopeSchema.safeParse(input)
  if (!envelope.success) {
    throw new ElementsError(`malformed inka export\n${z.prettifyError(envelope.error)}`)
  }

  const elements: ArgumentElement[] = []
  const problems: string[] = []
  envelope.data.forEach((element, index) => {
    const looksLikeEdge = 'source' in element.data || 'target' in element.data
    const schema = looksLikeEdge ? edgeDataSchema : nodeDataSchema
    const result = schema.safeParse(element.data)
    if (result.success) elements.push({ data: result.data })
    else {
      const kind = looksLikeEdge ? 'edge' : 'node'
      problems.push(`element ${index} (${kind}):\n${z.prettifyError(result.error)}`)
    }
  })

  if (problems.length > 0) {
    throw new ElementsError(`malformed inka export\n${problems.join('\n')}`)
  }
  return elements
}
