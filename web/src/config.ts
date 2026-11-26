/**
 * The wiring constants: where the bundle talks to `inka serve`.
 */

/** Endpoint answering with the page, relative to wherever the bundle is mounted. */
export const PAGE_URL = './api/page'

/** Endpoint saying who the server records decisions as, if it records. */
export const SESSION_URL = './api/session'

/** Endpoint a decision is posted to; present only on a recording server. */
export const DECISIONS_URL = './api/decisions'
