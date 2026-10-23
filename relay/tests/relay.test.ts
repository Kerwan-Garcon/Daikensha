import { beforeAll, describe, expect, it } from "vitest";

import { base64url, ProviderTokenSigner } from "../src/apns";
import { createHandler, type Env } from "../src/index";
import { buildApnsPayload } from "../src/payload";
import { MAX_BODY_BYTES, parseLiveActivityRequest, type LiveActivityRequest } from "../src/request";

const NOW = 1_800_000_000;
const TOKEN = "ab".repeat(32);

const validState = {
  pillId: "claude",
  agent: "Claude Code",
  color: "#ff8800",
  state: "working",
  statusText: "working · 3/7",
  tone: "neutral",
  stepIndex: 3,
  stepCount: 7,
  others: 0,
};

const validBody = { token: TOKEN, env: "development", event: "update", state: validState };

function parsed(body: unknown): LiveActivityRequest {
  const result = parseLiveActivityRequest(body);
  if (!result.ok) throw new Error(`expected a valid request, got ${result.error}`);
  return result.value;
}

describe("parseLiveActivityRequest", () => {
  it("accepts a well-formed request", () => {
    expect(parsed(validBody)).toEqual({ ...validBody, urgent: false });
  });

  it.each([
    ["body", null],
    ["body", [validBody]],
    ["token", { ...validBody, token: "not-hex" }],
    ["token", { ...validBody, token: "ab" }],
    ["env", { ...validBody, env: "staging" }],
    ["event", { ...validBody, event: "pause" }],
    ["state", { ...validBody, state: "x" }],
    ["state.agent", { ...validBody, state: { ...validState, agent: "x".repeat(61) } }],
    ["state.stepIndex", { ...validBody, state: { ...validState, stepIndex: 1.5 } }],
    ["state.others", { ...validBody, state: { ...validState, others: 1000 } }],
    ["state.since", { ...validBody, state: { ...validState, since: 12 } }],
    ["state.approval", { ...validBody, state: { ...validState, approval: "ABC" } }],
    ["dismissAfter", { ...validBody, dismissAfter: -1 }],
    ["dismissAfter", { ...validBody, dismissAfter: null }],
  ])("names %s when it is invalid", (field, body) => {
    expect(parseLiveActivityRequest(body)).toEqual({ ok: false, error: field });
  });

  it("drops unknown state fields, so nothing else rides along", () => {
    const request = parsed({ ...validBody, state: { ...validState, project: "secret-repo", since: null } });
    expect(request.state).not.toHaveProperty("project");
    expect(request.state).not.toHaveProperty("since");
  });
});

describe("buildApnsPayload", () => {
  it("starts an activity with an alert and the attributes type", () => {
    const { aps } = buildApnsPayload(parsed({ ...validBody, event: "start" }), NOW);
    expect(aps).toMatchObject({
      timestamp: NOW,
      event: "start",
      "stale-date": NOW + 900,
      "attributes-type": "MochiActivityAttributes",
      alert: { title: "Coucou", body: "Claude Code · working · 3/7" },
    });
  });

  it("alerts on an update only when urgent", () => {
    expect(buildApnsPayload(parsed(validBody), NOW).aps).not.toHaveProperty("alert");
    const urgent = buildApnsPayload(parsed({ ...validBody, urgent: true }), NOW).aps;
    expect(urgent["alert"]).toEqual({ title: "Claude Code", body: "working · 3/7" });
  });

  it("dismisses an ended activity after the requested delay", () => {
    const { aps } = buildApnsPayload(parsed({ ...validBody, event: "end", dismissAfter: 30 }), NOW);
    expect(aps["dismissal-date"]).toBe(NOW + 30);
  });

  it("keeps since and approval when present", () => {
    const approval = "f".repeat(64);
    const state = { ...validState, since: NOW, approval };
    const { aps } = buildApnsPayload(parsed({ ...validBody, state }), NOW);
    expect(aps["content-state"]).toEqual(state);
  });
});

describe("the Worker", () => {
  let env: Env;
  let publicKey: CryptoKey;

  beforeAll(async () => {
    // workers-types return the widest unions here; ECDSA always yields a pair and pkcs8 always bytes.
    const pair = (await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, [
      "sign",
      "verify",
    ])) as CryptoKeyPair;
    publicKey = pair.publicKey;
    const der = new Uint8Array((await crypto.subtle.exportKey("pkcs8", pair.privateKey)) as ArrayBuffer);
    const pem = `-----BEGIN PRIVATE KEY-----\n${btoa(String.fromCharCode(...der))}\n-----END PRIVATE KEY-----`;
    env = { APNS_KEY: pem, APNS_KEY_ID: "KEY123", APNS_TEAM_ID: "TEAM456", APNS_TOPIC: "topic.liveactivity" };
  });

  function harness(apnsStatus = 200) {
    const calls: { url: string; init: RequestInit }[] = [];
    const handler = createHandler({
      fetch: (input, init) => {
        calls.push({ url: String(input), init: init ?? {} });
        return Promise.resolve(new Response(apnsStatus === 200 ? "" : '{"reason":"BadDeviceToken"}', { status: apnsStatus }));
      },
      nowSeconds: () => NOW,
      signer: new ProviderTokenSigner(),
    });
    const send = (request: Request) => handler.fetch!(request as Request<unknown, IncomingRequestCfProperties>, env, {} as ExecutionContext);
    return { calls, send };
  }

  const post = (body: BodyInit, headers: HeadersInit = {}) =>
    new Request("https://relay.example/v1/live-activity", { method: "POST", body, headers, duplex: "half" } as RequestInit);

  it("answers its health check and 404s anything else", async () => {
    const { send } = harness();
    expect(await (await send(new Request("https://relay.example/"))).text()).toBe("Coucou relay");
    expect((await send(new Request("https://relay.example/v1/other", { method: "POST" }))).status).toBe(404);
  });

  it("forwards a valid push to the sandbox with a verifiable ES256 token", async () => {
    const { calls, send } = harness();
    const response = await send(post(JSON.stringify(validBody)));
    expect(response.status).toBe(200);
    expect(calls).toHaveLength(1);
    const [call] = calls;
    expect(call?.url).toBe(`https://api.sandbox.push.apple.com/3/device/${TOKEN}`);
    const headers = new Headers(call?.init.headers);
    expect(headers.get("apns-priority")).toBe("5");
    expect(headers.get("apns-topic")).toBe("topic.liveactivity");

    const [header, claims, signature] = (headers.get("authorization") ?? "").replace("bearer ", "").split(".");
    const decode = (part = "") => Uint8Array.from(atob(part.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
    expect(JSON.parse(new TextDecoder().decode(decode(claims)))).toEqual({ iss: "TEAM456", iat: NOW });
    const valid = await crypto.subtle.verify(
      { name: "ECDSA", hash: "SHA-256" },
      publicKey,
      decode(signature),
      new TextEncoder().encode(`${header}.${claims}`),
    );
    expect(valid).toBe(true);
  });

  it("reuses the provider token across requests", async () => {
    const { calls, send } = harness();
    await send(post(JSON.stringify(validBody)));
    await send(post(JSON.stringify({ ...validBody, event: "start" })));
    const auth = calls.map((c) => new Headers(c.init.headers).get("authorization"));
    expect(auth[0]).toBe(auth[1]);
  });

  it("rejects a declared oversize body before reading it", async () => {
    const { calls, send } = harness();
    const response = await send(post("{}", { "content-length": String(MAX_BODY_BYTES + 1) }));
    expect(response.status).toBe(413);
    expect(calls).toHaveLength(0);
  });

  it("rejects an oversize streamed body that declares no length", async () => {
    const { calls, send } = harness();
    const chunk = new TextEncoder().encode(" ".repeat(1024));
    let sent = 0;
    const stream = new ReadableStream<Uint8Array>({
      pull(controller) {
        if (sent++ < 64) controller.enqueue(chunk);
        else controller.close();
      },
    });
    const response = await send(post(stream));
    expect(response.status).toBe(413);
    expect(sent).toBeLessThan(64);
    expect(calls).toHaveLength(0);
  });

  it("returns 400 on bad JSON and names an invalid field", async () => {
    const { send } = harness();
    expect(await (await send(post("{"))).text()).toBe("bad json");
    const response = await send(post(JSON.stringify({ ...validBody, env: "x" })));
    expect([response.status, await response.text()]).toEqual([400, "env"]);
  });

  it("passes 410 through so the Mac drops the token, and maps other APNs errors to 502", async () => {
    expect((await harness(410).send(post(JSON.stringify(validBody)))).status).toBe(410);
    const bad = await harness(400).send(post(JSON.stringify(validBody)));
    expect(bad.status).toBe(502);
    expect(bad.headers.get("x-apns-status")).toBe("400");
  });
});

describe("base64url", () => {
  it("has no padding or URL-unsafe characters", () => {
    expect(base64url(new Uint8Array([0xfb, 0xff, 0xfe]))).toBe("-__-");
  });
});
