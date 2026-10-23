// Readers for untrusted JSON (integration payloads, events, localStorage):
// they turn `unknown` into the type a view needs, never into "[object Object]".

/** A string for display: strings as they are, numbers and booleans spelled out, anything else `fallback`. */
export function text(value: unknown, fallback = ""): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return fallback;
}

/** A message for whatever a promise rejected with. */
export function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return text(error, "unknown error");
}
