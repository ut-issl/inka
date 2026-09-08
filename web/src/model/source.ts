/**
 * Where the argument JSON comes from.
 *
 * `inka serve` may either inject the export into the page it serves or answer
 * a fetch endpoint with it; the viewer supports both and prefers the injected
 * payload, so a served page renders without a second round trip and a bundle
 * opened against a plain endpoint still works.
 *
 * Injected form, anywhere in the served HTML:
 *
 * ```html
 * <script type="application/json" id="inka-elements">[ … ]</script>
 * ```
 */

import { parseElements } from './elements'
import { buildView, type ArgumentView } from './view'

/** Element id `inka serve` injects the export under. */
export const INJECTED_ELEMENT_ID = 'inka-elements'

/** Raised when the argument could not be obtained at all. */
export class SourceError extends Error {
  override readonly name = 'SourceError'
}

/** Reads an injected payload, or null when the page carries none. */
export function readInjected(doc: Document = document): unknown {
  const script = doc.getElementById(INJECTED_ELEMENT_ID)
  const text = script?.textContent?.trim()
  if (!text) return null
  try {
    return JSON.parse(text) as unknown
  } catch (error) {
    throw new SourceError(`injected ${INJECTED_ELEMENT_ID} is not JSON: ${message(error)}`)
  }
}

/** Fetches the export from `inka serve`. */
export async function fetchElements(url: string, signal?: AbortSignal): Promise<unknown> {
  let response: Response
  try {
    response = await fetch(url, { signal: signal ?? null, headers: { Accept: 'application/json' } })
  } catch (error) {
    throw new SourceError(`could not reach \`${url}\`: ${message(error)}`)
  }
  if (!response.ok) {
    throw new SourceError(`\`${url}\` answered ${response.status} ${response.statusText}`)
  }
  try {
    return (await response.json()) as unknown
  } catch (error) {
    throw new SourceError(`\`${url}\` did not answer JSON: ${message(error)}`)
  }
}

/**
 * Loads the shared view: the injected payload when the page has one, the
 * endpoint otherwise.
 *
 * @throws {SourceError} when the payload cannot be obtained, and
 * `ElementsError` / `ViewError` when it is not a usable argument.
 */
export async function loadView(url: string, signal?: AbortSignal): Promise<ArgumentView> {
  const injected = readInjected()
  const payload = injected ?? (await fetchElements(url, signal))
  return buildView(parseElements(payload))
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
