/**
 * A description: its rendering, and one control away, its source.
 *
 * The source is the interpolated, normalized text the view hash is computed
 * from. Rendering shows every word of it but folds whitespace and line
 * breaks, which are contract content too; the source is where they are read.
 */

import { useId, useState } from 'react'

import { neutralize } from '../text/neutralize'
import { Markdown } from './Markdown'

export function Description({ text }: { text: string }) {
  const [source, setSource] = useState(false)
  const id = useId()
  return (
    <div className="description">
      <div id={id} className="description__body">
        {source ? <pre className="description__source">{neutralize(text)}</pre> : <Markdown text={text} />}
      </div>
      <button
        type="button"
        className="description__toggle"
        aria-pressed={source}
        aria-controls={id}
        onClick={() => setSource((shown) => !shown)}
      >
        {source ? 'Rendered' : 'Source'}
      </button>
    </div>
  )
}
