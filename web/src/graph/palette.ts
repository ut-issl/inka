/**
 * Namespace colours.
 *
 * Colours are presentation, not invariant: the only rule is that a namespace
 * keeps one colour everywhere it appears — badge, list header, list entry —
 * which holds because the assignment follows the view's namespace order.
 */

const PALETTE = [
  '#6366f1',
  '#0ea5e9',
  '#f59e0b',
  '#ef4444',
  '#10b981',
  '#a855f7',
  '#14b8a6',
  '#e11d48',
] as const

/** Fallback for a namespace absent from the view (should not happen). */
const UNKNOWN = '#94a3b8'

export type NamespaceColors = ReadonlyMap<string, string>

/** Assigns a colour per namespace, cycling the palette in the given order. */
export function namespaceColors(namespaces: readonly string[]): NamespaceColors {
  return new Map(
    namespaces.map((name, index) => [name, PALETTE[index % PALETTE.length] as string]),
  )
}

/** Colour of one namespace, never undefined so callers stay branch-free. */
export function colorOf(colors: NamespaceColors, namespace: string): string {
  return colors.get(namespace) ?? UNKNOWN
}
