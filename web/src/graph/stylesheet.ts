/**
 * The cytoscape stylesheet.
 *
 * What it encodes structurally: a node's shape and fill stand for its
 * discharge mode and the four are distinct; a node auto-sizes to its label;
 * the label is the node id (read straight from `data(id)`, so a description
 * can never leak into the node); every edge has the same weight and carries
 * its arrowhead at the child.
 *
 * The particular colours, paddings and font sizes are tuning parameters.
 */

import type { StylesheetJson } from 'cytoscape'

const MODE_STYLE = {
  objective: { shape: 'round-rectangle', fill: '#dbeafe', border: '#3b82f6' },
  structure: { shape: 'rectangle', fill: '#f1f5f9', border: '#64748b' },
  bind: { shape: 'round-rectangle', fill: '#dcfce7', border: '#22c55e' },
  fiat: { shape: 'hexagon', fill: '#fef9c3', border: '#eab308' },
} as const

/** Mode swatches for the legend and the node list, from the same source. */
export const MODE_COLORS: Readonly<Record<keyof typeof MODE_STYLE, string>> = {
  objective: MODE_STYLE.objective.border,
  structure: MODE_STYLE.structure.border,
  bind: MODE_STYLE.bind.border,
  fiat: MODE_STYLE.fiat.border,
}

const EDGE_COLOR = '#475569'

export function stylesheet(): StylesheetJson {
  const modes = Object.entries(MODE_STYLE).map(([mode, style]) => ({
    selector: `node[mode = "${mode}"]`,
    style: {
      shape: style.shape,
      'background-color': style.fill,
      'border-color': style.border,
    },
  }))

  return [
    {
      selector: 'node',
      style: {
        label: 'data(id)',
        'font-size': 16,
        'font-family': 'ui-sans-serif, system-ui, sans-serif',
        'text-valign': 'center',
        'text-halign': 'center',
        // Auto-size to the label rather than wrapping it into a fixed box.
        // cytoscape 3.34 prints a deprecation notice for this value but offers
        // no replacement; sizing to the label is the invariant, so it stays.
        width: 'label',
        height: 'label',
        padding: '16px',
        'border-width': 2,
        color: '#0f172a',
      },
    },
    ...modes,
    {
      selector: 'edge',
      style: {
        // One weight for every edge: an edge carries no emphasis of its own.
        width: 2,
        'line-color': EDGE_COLOR,
        'target-arrow-color': EDGE_COLOR,
        'target-arrow-shape': 'triangle',
        'arrow-scale': 1.1,
        'curve-style': 'bezier',
      },
    },
    {
      // A `children:` entry with a comment: dashed, so it reads as "hover me"
      // without taking on any weight.
      selector: 'edge[?commented]',
      style: { 'line-style': 'dashed' },
    },
    {
      // Emphasis only: a thicker dark rim, so shape and fill keep saying
      // "mode" and nothing else.
      selector: 'node.focused',
      style: { 'border-width': 5, 'border-color': '#0f172a' },
    },
  ] as StylesheetJson
}
