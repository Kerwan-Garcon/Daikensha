// APNs provider authentication: an ES256 JWT signed with the team's key.

import type { Env } from "./env";

/** Apple wants the token refreshed at most every 60 minutes and at least every 20; 50 sits between. */
const TOKEN_LIFETIME_SECONDS = 50 * 60;

interface CachedToken {
  readonly jwt: string;
  readonly issuedAt: number;
  readonly keyId: string;
}

/** Signs provider tokens and reuses each one for its lifetime (one per Worker isolate). */
export class ProviderTokenSigner {
  #cached: CachedToken | null = null;

  async token(env: Env, nowSeconds: number): Promise<string> {
    const cached = this.#cached;
    if (cached && cached.keyId === env.APNS_KEY_ID && nowSeconds - cached.issuedAt < TOKEN_LIFETIME_SECONDS) {
      return cached.jwt;
    }
    const jwt = await signProviderToken(env, nowSeconds);
    this.#cached = { jwt, issuedAt: nowSeconds, keyId: env.APNS_KEY_ID };
    return jwt;
  }
}

export async function signProviderToken(env: Env, nowSeconds: number): Promise<string> {
  const header = base64url(JSON.stringify({ alg: "ES256", kid: env.APNS_KEY_ID }));
  const claims = base64url(JSON.stringify({ iss: env.APNS_TEAM_ID, iat: nowSeconds }));
  const key = await crypto.subtle.importKey(
    "pkcs8",
    pemToDer(env.APNS_KEY),
    { name: "ECDSA", namedCurve: "P-256" },
    false,
    ["sign"],
  );
  const signature = await crypto.subtle.sign(
    { name: "ECDSA", hash: "SHA-256" },
    key,
    new TextEncoder().encode(`${header}.${claims}`),
  );
  return `${header}.${claims}.${base64url(new Uint8Array(signature))}`;
}

export const apnsHost = (environment: "development" | "production"): string =>
  environment === "production" ? "api.push.apple.com" : "api.sandbox.push.apple.com";

export function pemToDer(pem: string): ArrayBuffer {
  const b64 = pem.replace(/-----[^-]+-----/g, "").replace(/\s+/g, "");
  return Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)).buffer;
}

export function base64url(input: string | Uint8Array): string {
  const bytes = typeof input === "string" ? new TextEncoder().encode(input) : input;
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
