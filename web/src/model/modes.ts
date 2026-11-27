/**
 * The four discharge modes, in the order the legend lists them.
 *
 * Which modes exist is the server's to say: {@link Mode} is generated from the
 * Rust definition. This list only fixes an order to show them in, and it does
 * not compile unless it names every mode exactly as the server spells it.
 */

import type { Mode } from '../generated/Mode'

export const DISCHARGE_MODES = ['objective', 'structure', 'bind', 'fiat'] as const satisfies readonly Mode[]

/** Fails to compile when the server knows a mode this list leaves out. */
type Unlisted = Exclude<Mode, (typeof DISCHARGE_MODES)[number]>
const _everyModeIsListed: [Unlisted] extends [never] ? true : never = true
void _everyModeIsListed
