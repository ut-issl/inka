/**
 * The application shell: one page from the server, two surfaces over it.
 *
 * The review surface walks the review queue one local view at a time; the
 * graph surface shows the whole argument as cards, explored from its roots. Both read the same page, so they
 * cannot disagree about what the project says.
 *
 * On a server started without an approver the whole application is read-only.
 * On one that records, the review surface can append one approval record per
 * decision — the only thing the Web UI can change. Nothing here edits the
 * argument, the values, the evidence, or an existing record.
 */

import { HashRouter, Navigate, NavLink, Route, Routes } from 'react-router'

import { PAGE_URL, SESSION_URL } from './config'
import type { Finding } from './generated/Finding'
import type { ReviewerSession } from './generated/ReviewerSession'
import type { Snapshot } from './generated/Snapshot'
import { GraphSurface } from './graph/GraphSurface'
import { useSession } from './hooks/useSession'
import { usePage } from './hooks/useView'
import { ReviewSurface } from './review/ReviewSurface'

export function App() {
  const { state, reload } = usePage(PAGE_URL)
  const reviewer = useSession(SESSION_URL)

  if (state.status === 'loading') {
    return <p className="notice">Loading the argument…</p>
  }
  if (state.status === 'error') {
    return (
      <div className="notice notice--error" role="alert">
        <p>Could not load the argument.</p>
        <pre>{state.message}</pre>
      </div>
    )
  }
  if (state.status === 'rejected') {
    return <Findings findings={state.findings} />
  }
  // The hash carries the route, so the bundle works wherever it is mounted
  // and the server needs to know nothing about the surfaces.
  return (
    <HashRouter>
      <div className="shell">
        <Masthead snapshot={state.snapshot} reviewer={reviewer} />
        <Routes>
          <Route
            path="/review/:id?"
            element={
              <ReviewSurface
                snapshot={state.snapshot}
                nodes={state.view.nodes}
                reviewer={reviewer}
                onReload={reload}
              />
            }
          />
          <Route path="/graph/:id?" element={<GraphSurface snapshot={state.snapshot} nodes={state.view.nodes} />} />
          <Route path="*" element={<Navigate to="/review" replace />} />
        </Routes>
      </div>
    </HashRouter>
  )
}

/**
 * The surfaces, who decisions are recorded as, and the count `inka eval`
 * prints — always in sight.
 */
function Masthead({ snapshot, reviewer }: { snapshot: Snapshot; reviewer: ReviewerSession | null }) {
  return (
    <header className="masthead">
      <span className="masthead__name">inka</span>
      <nav className="masthead__surfaces" aria-label="Surfaces">
        <NavLink to="/review" className="masthead__surface">
          Review
        </NavLink>
        <NavLink to="/graph" className="masthead__surface">
          Graph
        </NavLink>
      </nav>
      <span className="masthead__reviewer">
        {reviewer ? (
          <>
            recording as <strong>{reviewer.approver}</strong>
          </>
        ) : (
          'read-only'
        )}
      </span>
      <span className="masthead__count">
        established {snapshot.established}/{snapshot.total}
      </span>
    </header>
  )
}

/**
 * A project that does not check, as the findings `inka check` reports. There
 * is no argument to draw until they are fixed; reloading the page asks again.
 */
function Findings({ findings }: { findings: readonly Finding[] }) {
  return (
    <div className="notice notice--error" role="alert">
      <p>The project does not check, so there is no argument to show.</p>
      <ul className="findings">
        {findings.map((finding, index) => (
          <li key={index} className={`findings__item findings__item--${finding.severity}`}>
            <span className="findings__severity">{finding.severity}</span>
            {finding.location ? (
              <code className="findings__location">
                {finding.location.path}:{finding.location.line}:{finding.location.column}
              </code>
            ) : null}
            <span className="findings__message">{finding.message}</span>
          </li>
        ))}
      </ul>
    </div>
  )
}
