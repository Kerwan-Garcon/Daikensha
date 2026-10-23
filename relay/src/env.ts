/** Worker bindings: wrangler.toml `[vars]` and the secrets set with `wrangler secret put`. */
export interface Env {
  /** Contents of the AuthKey_XXXX.p8 file (secret). */
  readonly APNS_KEY: string;
  /** The key's ID (secret). */
  readonly APNS_KEY_ID: string;
  readonly APNS_TEAM_ID: string;
  readonly APNS_TOPIC: string;
}
