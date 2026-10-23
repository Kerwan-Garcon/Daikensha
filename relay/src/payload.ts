// The APNs payload, built by the relay itself from the validated state only, so
// the relay can't be used to send arbitrary notifications.

import { STATE_NUMBER_KEYS, STATE_STRING_KEYS, type LiveActivityRequest } from "./request";

/** If the Mac goes quiet (asleep, offline), the iPhone greys the activity out after this. */
const STALE_AFTER_SECONDS = 15 * 60;

export interface ApnsPayload {
  readonly aps: Readonly<Record<string, unknown>>;
}

/** `nowSeconds` is injected so the payload is a pure function of its inputs. */
export function buildApnsPayload(request: LiveActivityRequest, nowSeconds: number): ApnsPayload {
  const { state } = request;
  // Only the known fields, in a fixed order, so nothing else rides along.
  const contentState: Record<string, unknown> = {};
  for (const key of [...STATE_STRING_KEYS, ...STATE_NUMBER_KEYS]) contentState[key] = state[key];
  // When Mochi left for the iPhone: the activity counts the time from it.
  if (state.since !== undefined) contentState["since"] = state.since;
  // The pending command's fingerprint (a hash), for Allow / Deny on the Lock Screen.
  if (state.approval !== undefined) contentState["approval"] = state.approval;

  const aps: Record<string, unknown> = {
    timestamp: nowSeconds,
    event: request.event,
    "content-state": contentState,
    "stale-date": nowSeconds + STALE_AFTER_SECONDS,
  };
  switch (request.event) {
    case "start":
      aps["attributes-type"] = "MochiActivityAttributes";
      aps["attributes"] = {};
      // iOS requires an alert to start a Live Activity from a push.
      aps["alert"] = { title: "Coucou", body: `${state.agent} · ${state.statusText}` };
      break;
    case "update":
      // Waiting for your OK or a question: the Dynamic Island opens and the Lock
      // Screen lights up, with Allow / Deny right there. No sound: the approval
      // notification already makes one.
      if (request.urgent) aps["alert"] = { title: state.agent, body: state.statusText };
      break;
    case "end":
      aps["dismissal-date"] = nowSeconds + (request.dismissAfter ?? 0);
      break;
  }
  return { aps };
}

/** 10 is limited by an iOS budget: only for start, end and waiting-for-you. */
export const apnsPriority = (request: LiveActivityRequest): "10" | "5" =>
  request.urgent || request.event !== "update" ? "10" : "5";
