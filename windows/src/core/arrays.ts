// Checked array access for code whose indices are bounded by construction
// (loops over `length`, a length tested just above). With
// `noUncheckedIndexedAccess`, `items[i]` is `T | undefined`; these helpers state
// the invariant once and fail loudly if it is ever broken, instead of letting an
// `undefined` reach a canvas call and draw nothing.

/** `items[index]`, for an index the caller has already bounds-checked. */
export function itemAt<T>(items: ArrayLike<T>, index: number): T {
  if (index < 0 || index >= items.length) {
    throw new RangeError(`index ${index} out of bounds (length ${items.length})`);
  }
  return items[index] as T;
}

/** The last element of a non-empty array. */
export function lastOf<T>(items: ArrayLike<T>): T {
  return itemAt(items, items.length - 1);
}

/** The element at `index` wrapped into range, negative indices included (palettes, carousels). */
export function cyclicAt<T>(items: ArrayLike<T>, index: number): T {
  const n = items.length;
  return itemAt(items, ((index % n) + n) % n);
}
