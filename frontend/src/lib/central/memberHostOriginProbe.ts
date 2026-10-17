import { MEMBER_HOST_ORIGIN_PROBE_TIMEOUT_MS } from "./memberRoomLive";

export type MemberHostOriginIdentity = {
  server_id: string;
  host_key_fingerprint: string;
  host_public_key_jwk: JsonWebKey;
};

export type MemberHostOriginProbeResult = MemberHostOriginIdentity | null;

export type MemberHostOriginProbeFetch =
  (input: string, init?: RequestInit) => Promise<Response>;

// Unsigned public probe of a stored friend origin: GET <origin>/api/server-info
// with a short timeout and no credentials/central. Returns the host identity
// only when the same host answers (server_id + host key fingerprint both
// match the anchor); any network failure, timeout, invalid payload or
// identity mismatch returns null so the caller treats the host as still down
// and waits for the slow schedule instead of spending central.
export async function probeMemberHostOrigin(
  origin: string,
  expected: { server_id: string; host_key_fingerprint: string },
  fetchFn: MemberHostOriginProbeFetch = fetch,
  timeoutMs: number = MEMBER_HOST_ORIGIN_PROBE_TIMEOUT_MS,
): Promise<MemberHostOriginProbeResult> {
  let url: URL;
  try {
    url = new URL(`${origin}/api/server-info`);
  } catch {
    return null;
  }
  if (url.protocol !== "https:" || url.origin !== origin) return null;
  if (!expected.server_id || !expected.host_key_fingerprint) return null;
  const bounded = Number.isFinite(timeoutMs) && timeoutMs > 0
    ? timeoutMs
    : MEMBER_HOST_ORIGIN_PROBE_TIMEOUT_MS;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), bounded);
  try {
    const response = await fetchFn(url.toString(), {
      method: "GET",
      cache: "no-store",
      credentials: "omit",
      redirect: "error",
      referrerPolicy: "no-referrer",
      signal: controller.signal,
    });
    if (!response.ok) return null;
    let value: unknown = null;
    try {
      value = await response.json();
    } catch {
      return null;
    }
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const record = value as Record<string, unknown>;
    const server_id = typeof record.server_id === "string" ? record.server_id : "";
    const fingerprint = typeof record.host_key_fingerprint === "string" ? record.host_key_fingerprint : "";
    const jwk = record.host_public_key_jwk;
    if (server_id !== expected.server_id || fingerprint !== expected.host_key_fingerprint) return null;
    if (!jwk || typeof jwk !== "object" || Array.isArray(jwk)) return null;
    return { server_id, host_key_fingerprint: fingerprint, host_public_key_jwk: jwk as JsonWebKey };
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}
