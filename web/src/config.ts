/**
 * Presentation parameters and the wiring constants.
 *
 * Everything here is tuning — spacing, zoom thresholds, animation — except
 * the three URLs, which are where the bundle talks to `inka serve`.
 */

/** Endpoint answering with the page, relative to wherever the bundle is mounted. */
export const PAGE_URL = './api/page'

/** Endpoint saying who the server records decisions as, if it records. */
export const SESSION_URL = './api/session'

/** Endpoint a decision is posted to; present only on a recording server. */
export const DECISIONS_URL = './api/decisions'

/** Zoom below which namespace badges are hidden. */
export const BADGE_MIN_ZOOM = 0.75

export const LAYOUT = {
  /** Vertical distance between layered ranks. */
  rankSep: 90,
  /** Horizontal separation the layered pass aims for. */
  nodeSep: 45,
  /** Gap left between neighbouring nodes when a rank is re-spaced. */
  gap: 48,
  /** Vertical distance between the three node-focus rows. */
  rowGap: 260,
  /** Padding kept around the graph when it is fitted to the window. */
  fitPadding: 44,
  /** Aspect-ratio match tolerance, and the passes allowed to reach it. */
  tolerance: 0.02,
  iterations: 6,
  /** Duration of the fit animation; 0 while resizing. */
  fitDuration: 280,
} as const
