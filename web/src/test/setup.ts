import '@testing-library/jest-dom/vitest'

// jsdom implements no layout, so scrolling has no meaning there; the viewer
// only ever uses it to keep a focused list entry in sight.
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {}
}
