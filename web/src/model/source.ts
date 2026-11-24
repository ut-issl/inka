/**
 * Where the page comes from: one `GET` to `inka serve`.
 *
 * The answer is a reading of the repository at the moment of the request, so
 * it is fetched, never cached and never embedded in the served HTML. Its shape
 * is defined in Rust and its TypeScript type is generated from that definition
 * (`src/generated/`); the bundle is embedded in the `inka` that answers, so
 * the two always match and nothing here re-states or re-checks the shape.
 */

import type { Page } from '../generated/Page'

/** Raised when the page could not be obtained at all. */
export class SourceError extends Error {
  override readonly name = 'SourceError'
}

/**
 * Fetches the page from `inka serve`.
 *
 * @throws {SourceError} when the server cannot be reached or does not answer
 * with a page.
 */
export async function loadPage(url: string, signal?: AbortSignal): Promise<Page> {
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
    return (await response.json()) as Page
  } catch (error) {
    throw new SourceError(`\`${url}\` did not answer JSON: ${message(error)}`)
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}
