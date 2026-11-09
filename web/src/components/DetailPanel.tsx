/**
 * The detail panel: what the current focus is about.
 *
 * A focused node shows its contract, its evidence target, its children (each
 * with the comment on its entry) and its deadline, each element with the
 * comment attached to it; a
 * focused namespace shows its size and its one-hop surroundings; the whole
 * view names the objective roots the argument hangs from. Nothing here links
 * to an action — the viewer only reads the argument.
 */

import type { Focus, FocusSets } from '../model/focus'
import { nodeOf, rootsOf, type ArgumentNode, type ArgumentView } from '../model/view'

interface Props {
  readonly view: ArgumentView
  readonly focus: Focus
  readonly sets: FocusSets
}

export function DetailPanel({ view, focus, sets }: Props) {
  if (focus.kind === 'node' && view.nodes.has(focus.id)) {
    return <NodeDetail view={view} id={focus.id} />
  }
  if (focus.kind === 'namespace') {
    return <NamespaceDetail name={focus.name} sets={sets} />
  }
  return <WholeViewDetail view={view} />
}

function NodeDetail({ view, id }: { view: ArgumentView; id: string }) {
  const node = nodeOf(view, id)
  return (
    <section className="detail">
      <h2 className="detail__title">{node.id}</h2>
      <p className="detail__meta">
        {node.mode} · {node.namespace}
        {node.tags.map((tag) => (
          <span key={tag} className="detail__tag">
            #{tag}
          </span>
        ))}
      </p>
      <Comment text={node.comments.block} />

      {node.evidence ? (
        <p className="detail__row">
          <span className="detail__label">Verified by</span> {node.evidence.kind}:{' '}
          <code>{node.evidence.target}</code>
          <Comment text={node.comments[node.evidence.kind]} />
        </p>
      ) : null}

      <ChildrenRow view={view} node={node} />
      <DeadlineRows node={node} />

      <p className="detail__label">Contract</p>
      <p className="detail__contract">{node.description}</p>
    </section>
  )
}

/**
 * The children, each with the comment on its `children:` entry — the same
 * comment hovering the edge reveals, readable here without a pointer.
 */
function ChildrenRow({ view, node }: { view: ArgumentView; node: ArgumentNode }) {
  if (node.children.length === 0) return null
  return (
    <div className="detail__row">
      <span className="detail__label">Children</span>
      <Comment text={node.comments.children} />
      <ul className="detail__children">
        {node.children.map((child) => (
          <li key={child}>
            {child}
            <Comment text={entryComment(view, node.id, child)} />
          </li>
        ))}
      </ul>
    </div>
  )
}

/** `until:` and `promote:` on rows of their own, each under its comment. */
function DeadlineRows({ node }: { node: ArgumentNode }) {
  if (!node.deadline) return null
  return (
    <>
      <p className="detail__row detail__row--deadline">
        <span className="detail__label">Until</span> {node.deadline.until}
        <Comment text={node.comments.until} />
      </p>
      {node.deadline.promote ? (
        <p className="detail__row detail__row--deadline">
          <span className="detail__label">Promote to</span> {node.deadline.promote}
          <Comment text={node.comments.promote} />
        </p>
      ) : null}
    </>
  )
}

function entryComment(view: ArgumentView, parent: string, child: string): string | undefined {
  return view.edges.find((edge) => edge.source === parent && edge.target === child)?.comment
}

/**
 * A comment attached to the element it follows, or nothing. Set apart from
 * the row's own text for assistive technology as well as visually.
 */
function Comment({ text }: { text: string | undefined }) {
  if (text === undefined) return null
  return (
    <span className="detail__comment">
      <span className="visually-hidden">Comment: </span>
      {text}
    </span>
  )
}

function NamespaceDetail({ name, sets }: { name: string; sets: FocusSets }) {
  const neighbours = sets.parents.size + sets.children.size
  return (
    <section className="detail">
      <h2 className="detail__title">{name}</h2>
      <p className="detail__meta">namespace</p>
      <p className="detail__contract">
        {sets.primary.size} node{sets.primary.size === 1 ? '' : 's'} in this namespace, plus{' '}
        {neighbours} one hop away ({sets.parents.size} above, {sets.children.size} below).
      </p>
    </section>
  )
}

function WholeViewDetail({ view }: { view: ArgumentView }) {
  const roots = rootsOf(view)
  return (
    <section className="detail">
      <h2 className="detail__title">Whole view</h2>
      <p className="detail__meta">
        {view.nodes.size} nodes · {view.edges.length} edges · {view.groups.length} namespaces
      </p>
      {roots.length > 0 ? (
        <p className="detail__contract">
          Objective{roots.length === 1 ? '' : 's'} on top: {roots.map((root) => root.id).join(', ')}.
          Click a node or a namespace badge to focus; Esc restores this view.
        </p>
      ) : (
        <p className="detail__contract">Click a node or a namespace badge to focus; Esc restores this view.</p>
      )}
    </section>
  )
}
