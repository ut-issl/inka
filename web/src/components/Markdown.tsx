/**
 * A description, rendered as CommonMark.
 *
 * Rendering is for reading and must not hide or alter contract content:
 *
 * - raw HTML in a description is shown as the text it is, never interpreted;
 * - no remote resource is loaded — an image is shown as its alternative text
 *   and its destination;
 * - a link shows where it leads, beside its text.
 *
 * The parsing and the safe rendering are the library's; this file only says
 * how those three cases are drawn. Text is neutralized first, as everywhere.
 */

import ReactMarkdown, { type Components } from 'react-markdown'

import { neutralize } from '../text/neutralize'

const components: Components = {
  img({ alt, src }) {
    return (
      <span className="md-image">
        <span className="md-image__label">image</span>
        {alt ? <span className="md-image__alt">{alt}</span> : null}
        {typeof src === 'string' && src ? <code className="md-destination">{src}</code> : null}
      </span>
    )
  },
  a({ href, children }) {
    return (
      <>
        <a href={href} target="_blank" rel="noreferrer noopener">
          {children}
        </a>
        {href ? <code className="md-destination">{href}</code> : null}
      </>
    )
  },
}

export function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown components={components}>{neutralize(text)}</ReactMarkdown>
    </div>
  )
}
