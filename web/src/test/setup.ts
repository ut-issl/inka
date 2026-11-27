import '@testing-library/jest-dom/vitest'

// jsdom implements no layout, so scrolling has no meaning there; the viewer
// only ever uses it to keep a focused list entry in sight.
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {}
}

// Nor does it observe sizes. The resizable panes ask for a `ResizeObserver`;
// one that never reports is true to an environment where nothing has a size.
if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
}
