// Lookups for elements the code knows are there. Dependency-free, so the small
// windows (pill, circle) can use them without pulling in the island's modules.

/**
 * An element the page's HTML is known to contain (a window's root, a template's
 * part). Throws with the selector when the markup and the code disagree,
 * instead of a TypeError on `null` somewhere later.
 */
export function requireElement(id: string): HTMLElement {
  const el = document.getElementById(id);
  if (!el) throw new Error(`#${id} is missing from the page`);
  return el;
}

/** The first `selector` match inside `root`, which the caller built to contain it. */
export function requireChild<K extends keyof HTMLElementTagNameMap>(root: ParentNode, selector: K): HTMLElementTagNameMap[K];
export function requireChild<K extends keyof SVGElementTagNameMap>(root: ParentNode, selector: K): SVGElementTagNameMap[K];
export function requireChild(root: ParentNode, selector: string): HTMLElement;
export function requireChild(root: ParentNode, selector: string): Element {
  const el = root.querySelector(selector);
  if (!el) throw new Error(`${selector} is missing`);
  return el;
}

/** A canvas's 2D context; only a lost GPU process or an exhausted context pool refuses one. */
export function context2d(canvas: HTMLCanvasElement): CanvasRenderingContext2D {
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("no 2D canvas context");
  return ctx;
}
