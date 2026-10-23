// The one request the relay accepts, parsed from untrusted JSON into a type that
// is valid by construction. Everything downstream works on `LiveActivityRequest`
// and never re-checks a field.

/** Upper bound on a request body, enforced while reading (not only from Content-Length). */
export const MAX_BODY_BYTES = 4096;

export type ApnsEnvironment = "development" | "production";
export type LiveActivityEvent = "start" | "update" | "end";

export const STATE_STRING_KEYS = ["pillId", "agent", "color", "state", "statusText", "tone"] as const;
export const STATE_NUMBER_KEYS = ["stepIndex", "stepCount", "others"] as const;

type StateStringKey = (typeof STATE_STRING_KEYS)[number];
type StateNumberKey = (typeof STATE_NUMBER_KEYS)[number];

/** Mochi's state: no project name, command or path can ride along (see MochiActivityState.swift). */
export type ActivityState = Readonly<Record<StateStringKey, string>> &
  Readonly<Record<StateNumberKey, number>> & {
    /** When Mochi left for the iPhone (Unix seconds). */
    readonly since?: number;
    /** SHA-256 fingerprint of the command waiting for approval. */
    readonly approval?: string;
  };

export interface LiveActivityRequest {
  readonly token: string;
  readonly env: ApnsEnvironment;
  readonly event: LiveActivityEvent;
  readonly state: ActivityState;
  readonly urgent: boolean;
  /** "end" only: seconds the final state stays on the Lock Screen. */
  readonly dismissAfter?: number;
}

export type ParseResult<T> = { readonly ok: true; readonly value: T } | { readonly ok: false; readonly error: string };

const TOKEN = /^[0-9a-f]{32,400}$/i;
const SHA256_HEX = /^[0-9a-f]{64}$/;
const MAX_STATE_STRING = 60;
const MAX_STATE_NUMBER = 999;
const MIN_SINCE = 1_600_000_000;
const MAX_SINCE = 4_000_000_000;
const MAX_DISMISS_AFTER = 4 * 3600;

const isRecord = (value: unknown): value is Readonly<Record<string, unknown>> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const isIntegerIn = (value: unknown, min: number, max: number): value is number =>
  typeof value === "number" && Number.isInteger(value) && value >= min && value <= max;

/** `undefined` and `null` both mean "absent" for the optional fields. */
const isAbsent = (value: unknown): value is null | undefined => value === undefined || value === null;

const fail = (error: string): ParseResult<never> => ({ ok: false, error });

function parseState(raw: unknown): ParseResult<ActivityState> {
  if (!isRecord(raw)) return fail("state");
  const state: Record<string, string | number> = {};
  for (const key of STATE_STRING_KEYS) {
    const value = raw[key];
    if (typeof value !== "string" || value.length > MAX_STATE_STRING) return fail(`state.${key}`);
    state[key] = value;
  }
  for (const key of STATE_NUMBER_KEYS) {
    const value = raw[key];
    if (!isIntegerIn(value, 0, MAX_STATE_NUMBER)) return fail(`state.${key}`);
    state[key] = value;
  }
  const { since, approval } = raw;
  if (!isAbsent(since)) {
    if (!isIntegerIn(since, MIN_SINCE, MAX_SINCE)) return fail("state.since");
    state["since"] = since;
  }
  if (!isAbsent(approval)) {
    if (typeof approval !== "string" || !SHA256_HEX.test(approval)) return fail("state.approval");
    state["approval"] = approval;
  }
  // Every key above was checked, in the shape ActivityState declares.
  return { ok: true, value: state as ActivityState };
}

/** Validates an untrusted body. The error names the first offending field. */
export function parseLiveActivityRequest(body: unknown): ParseResult<LiveActivityRequest> {
  if (!isRecord(body)) return fail("body");
  const { token, env, event, urgent, dismissAfter } = body;
  if (typeof token !== "string" || !TOKEN.test(token)) return fail("token");
  if (env !== "development" && env !== "production") return fail("env");
  if (event !== "start" && event !== "update" && event !== "end") return fail("event");
  const state = parseState(body["state"]);
  if (!state.ok) return state;
  if (dismissAfter !== undefined && !isIntegerIn(dismissAfter, 0, MAX_DISMISS_AFTER)) return fail("dismissAfter");
  return {
    ok: true,
    value: {
      token,
      env,
      event,
      state: state.value,
      urgent: urgent === true,
      ...(dismissAfter === undefined ? {} : { dismissAfter }),
    },
  };
}

export type BodyResult = ParseResult<unknown> & { readonly status?: 400 | 413 };

/**
 * Reads a JSON body of at most `maxBytes`. The limit holds while streaming, so a
 * chunked request (no Content-Length) cannot make the Worker buffer more.
 */
export async function readJsonBody(request: Request, maxBytes = MAX_BODY_BYTES): Promise<BodyResult> {
  const declared = Number(request.headers.get("content-length") ?? "0");
  if (declared > maxBytes) return { ok: false, error: "too large", status: 413 };
  if (request.body === null) return { ok: false, error: "bad json", status: 400 };

  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > maxBytes) {
      await reader.cancel();
      return { ok: false, error: "too large", status: 413 };
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  try {
    return { ok: true, value: JSON.parse(new TextDecoder().decode(bytes)) as unknown };
  } catch {
    return { ok: false, error: "bad json", status: 400 };
  }
}
