/**
 * A description, rendered as CommonMark.
 *
 * Rendering is for reading and must not hide or alter contract content:
 *
 * - raw HTML in a description is shown as the text it is, never interpreted;
 * - no remote resource is loaded — an image is shown as its alternative text
 *   and its destination;
 * - a link shows where it leads, beside its text — the destination as it was
 *   written, even one that is not safe to follow, which is then not a link;
 * - the title of a link or an image is shown as text, not left to a tooltip.
 *
 * The parsing and the safe rendering are the library's; this file only says
 * how those cases are drawn. Text is neutralized twice: the source before it
 * is parsed, and every piece of text and every shown attribute after — where
 * a character reference such as `&#x202E;` has become the character it names.
 */

import ReactMarkdown, { defaultUrlTransform, type Components } from 'react-markdown'

import { neutralize } from '../text/neutralize'

/** A node of the tree the description is parsed into, as far as this file reads it. */
interface TreeNode {
  type: string
  value?: unknown
  properties?: Record<string, unknown>
  children?: TreeNode[]
}

/**
 * Neutralizes the parsed description: every text node, and every string
 * attribute, after character references have been decoded.
 */
function neutralizeParsed() {
  const visit = (node: TreeNode): void => {
    if (typeof node.value === 'string') node.value = neutralize(node.value)
    for (const [name, value] of Object.entries(node.properties ?? {})) {
      if (typeof value === 'string') (node.properties as Record<string, unknown>)[name] = neutralize(value)
    }
    node.children?.forEach(visit)
  }
  return visit
}

/** The title a link or an image carries, shown as the text it is. */
function Title({ title }: { title: unknown }) {
  if (typeof title !== 'string' || title === '') return null
  return <span className="md-title">“{title}”</span>
}

const components: Components = {
  img({ alt, src, title }) {
    return (
      <span className="md-image">
        <span className="md-image__label">image</span>
        {alt ? <span className="md-image__alt">{alt}</span> : null}
        <Title title={title} />
        {typeof src === 'string' && src ? <code className="md-destination">{src}</code> : null}
      </span>
    )
  },
  a({ href, title, children }) {
    // The destination is shown as written. It is followed only when it is
    // safe to: what the library would have refused becomes plain text.
    const safe = typeof href === 'string' && href !== '' && defaultUrlTransform(href) !== ''
    return (
      <>
        {safe ? (
          <a href={href} target="_blank" rel="noreferrer noopener">
            {children}
          </a>
        ) : (
          <span className="md-unlinked">{children}</span>
        )}
        <Title title={title} />
        {href ? <code className="md-destination">{href}</code> : null}
      </>
    )
  },
}

/** Destinations reach the renderers as written; each decides what is safe. */
const asWritten = (url: string): string => url

export function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown components={components} rehypePlugins={[neutralizeParsed]} urlTransform={asWritten}>
        {neutralize(text)}
      </ReactMarkdown>
    </div>
  )
}
