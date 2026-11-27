/** Where a node is found on each surface. The hash carries the route. */

export type Surface = 'review' | 'graph'

/** A target on the review surface. */
export function reviewPath(id: string): string {
  return `/review/${encodeURIComponent(id)}`
}

/** A node selected on the graph surface. */
export function graphPath(id: string): string {
  return `/graph/${encodeURIComponent(id)}`
}

export function nodePath(surface: Surface, id: string): string {
  return surface === 'review' ? reviewPath(id) : graphPath(id)
}
