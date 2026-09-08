/** How to read the graph: the four discharge modes and the focus colours. */

import { DISCHARGE_MODES } from '../model/elements'
import { MODE_COLORS } from '../graph/stylesheet'

export function Legend() {
  return (
    <section className="legend">
      <p className="legend__row">
        {DISCHARGE_MODES.map((mode) => (
          <span key={mode} className="legend__mode">
            <span className="legend__dot" style={{ backgroundColor: MODE_COLORS[mode] }} />
            {mode}
          </span>
        ))}
      </p>
      <p className="legend__row legend__hint">
        A namespace is the coloured badge on each node — click one, or a list header, to focus it.
        In the list, <span className="legend__parent">parents</span> and{' '}
        <span className="legend__child">children</span> of the focus are marked.
      </p>
    </section>
  )
}
