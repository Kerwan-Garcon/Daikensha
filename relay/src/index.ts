// Coucou relay — a stateless Cloudflare Worker.
//
// The Mac app can't send Live Activity pushes itself: that needs the APNs key,
// which must never ship inside an app. The Mac posts Mochi's state here, the
// relay signs a token with the key (a Worker secret) and forwards the push to
// Apple. Nothing is stored and nothing is logged: no database, observability
// off. The state holds no project name, command or path (see
// MochiActivityState.swift), and the relay builds the push itself, so it can't
// be used to send arbitrary notifications.
//
//   request.ts  untrusted JSON → LiveActivityRequest (bounded read, validation)
//   payload.ts  LiveActivityRequest → APNs payload (pure)
//   apns.ts     provider token (ES256 JWT) and APNs host

import { apnsHost, ProviderTokenSigner } from "./apns";
import type { Env } from "./env";
import { apnsPriority, buildApnsPayload } from "./payload";
import { parseLiveActivityRequest, readJsonBody } from "./request";

export type { Env } from "./env";

/** What the handler needs from the outside world; injected so tests need no network or clock. */
export interface Dependencies {
  readonly fetch: typeof fetch;
  readonly nowSeconds: () => number;
  readonly signer: ProviderTokenSigner;
}

export function createHandler(deps: Dependencies): ExportedHandler<Env> {
  return {
    async fetch(request, env) {
      const url = new URL(request.url);
      if (request.method === "GET" && url.pathname === "/") return text(200, "Coucou relay");
      if (request.method !== "POST" || url.pathname !== "/v1/live-activity") return text(404, "not found");

      const body = await readJsonBody(request);
      if (!body.ok) return text(body.status ?? 400, body.error);
      const parsed = parseLiveActivityRequest(body.value);
      if (!parsed.ok) return text(400, parsed.error);
      const push = parsed.value;

      const now = deps.nowSeconds();
      const jwt = await deps.signer.token(env, now);
      const apns = await deps.fetch(`https://${apnsHost(push.env)}/3/device/${push.token}`, {
        method: "POST",
        headers: {
          authorization: `bearer ${jwt}`,
          "apns-topic": env.APNS_TOPIC,
          "apns-push-type": "liveactivity",
          "apns-priority": apnsPriority(push),
          "content-type": "application/json",
        },
        body: JSON.stringify(buildApnsPayload(push, now)),
      });
      if (apns.status === 200) return text(200, "ok");
      // 400 BadDeviceToken / 410 Unregistered: the Mac drops the token.
      const reason = await apns.text();
      return new Response(reason || "{}", {
        status: apns.status === 410 ? 410 : 502,
        headers: { "content-type": "application/json", "x-apns-status": String(apns.status) },
      });
    },
  };
}

function text(status: number, message: string): Response {
  return new Response(message, { status, headers: { "content-type": "text/plain" } });
}

export default createHandler({
  fetch: (input, init) => fetch(input, init),
  nowSeconds: () => Math.floor(Date.now() / 1000),
  signer: new ProviderTokenSigner(),
});
