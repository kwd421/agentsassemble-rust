import { decodeCanonicalBase64Url, encodeBase64Url } from "../base64Url";
import { assertExactKeys, requiredString, strictRecord } from "../strictJsonContract";
import { exactCentralServerOrigin } from "./ownerConnect";
export { exactCentralServerOrigin as exactMemberOrigin } from "./ownerConnect";

const HOST_KEY = "agentsassemble.memberHandoff.v1:";
const CENTRAL_KEY = "agentsassemble.pendingMember.v1";
const HASH = /^[A-Za-z0-9_-]{43}$/;
export type MemberTargetRequest = {
  server_id: string; registration_epoch: string; challenge_hash: string; handoff_state: string;
};
export type MemberChallenge = Omit<MemberTargetRequest, "handoff_state"> & {
  challenge_id: string; expires_at: number;
};
export type MemberHandoff = MemberTargetRequest & {
  challenge_id: string; expires_at: number; invite_token: string; meeting_id: string;
};
export type MemberGrant = {
  grant_token: string; server_id: string; registration_epoch: string; expires_at: number;
  endpoint_origin: string; endpoint_generation: number;
};
export type MemberReturn = { record?: MemberHandoff; grant?: MemberGrant; error?: string; retry?: boolean };

function liveExpiry(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value <= Date.now() / 1000) {
    throw new Error("입장 요청이 만료됐어요. 다시 시도해 주세요.");
  }
  return value;
}

export function parseMemberTargetRequest(value: unknown): MemberTargetRequest {
  const r = strictRecord(value, "중앙 입장 요청");
  assertExactKeys(r, ["server_id", "registration_epoch", "challenge_hash", "handoff_state"], "중앙 입장 요청");
  const request = {
    server_id: requiredString(r, "server_id", "중앙 입장 요청"),
    registration_epoch: requiredString(r, "registration_epoch", "중앙 입장 요청"),
    challenge_hash: requiredString(r, "challenge_hash", "중앙 입장 요청"),
    handoff_state: requiredString(r, "handoff_state", "중앙 입장 요청"),
  };
  if (!HASH.test(request.challenge_hash) || !HASH.test(request.handoff_state) ||
      request.server_id.length > 200 || request.registration_epoch.length > 200) {
    throw new Error("중앙 입장 요청을 확인하지 못했어요.");
  }
  return request;
}

export function parseMemberChallenge(value: unknown): MemberChallenge {
  const r = strictRecord(value, "입장 확인");
  assertExactKeys(r, ["challenge_id", "challenge_hash", "server_id", "registration_epoch", "expires_at"], "입장 확인");
  const server_id = requiredString(r, "server_id", "입장 확인");
  const registration_epoch = requiredString(r, "registration_epoch", "입장 확인");
  const challenge_hash = requiredString(r, "challenge_hash", "입장 확인");
  if (server_id.length > 200 || registration_epoch.length > 200 || !HASH.test(challenge_hash)) {
    throw new Error("입장 확인 응답이 올바르지 않아요.");
  }
  return { server_id, registration_epoch, challenge_hash,
    challenge_id: requiredString(r, "challenge_id", "입장 확인"), expires_at: liveExpiry(r.expires_at) };
}

export function parseMemberGrant(value: unknown, expected: MemberTargetRequest): MemberGrant {
  const r = strictRecord(value, "중앙 입장권");
  assertExactKeys(r, ["grant_token", "server_id", "registration_epoch", "expires_at", "endpoint_origin", "endpoint_generation"], "중앙 입장권");
  if (r.server_id !== expected.server_id || r.registration_epoch !== expected.registration_epoch ||
      typeof r.endpoint_generation !== "number" || !Number.isSafeInteger(r.endpoint_generation) || r.endpoint_generation < 1 ||
      typeof r.grant_token !== "string" || !/^aamg1\.[A-Za-z0-9_-]{43}$/.test(r.grant_token)) {
    throw new Error("중앙 입장권이 요청한 서버와 일치하지 않아요.");
  }
  return { grant_token: r.grant_token, server_id: expected.server_id,
    registration_epoch: expected.registration_epoch, expires_at: liveExpiry(r.expires_at),
    endpoint_origin: exactCentralServerOrigin(requiredString(r, "endpoint_origin", "중앙 입장권")),
    endpoint_generation: r.endpoint_generation };
}

function encode(value: unknown): string {
  return encodeBase64Url(new TextEncoder().encode(JSON.stringify(value)));
}
function decode(value: string): unknown {
  if (value.length > 12_000) throw new Error("입장 응답이 너무 커요.");
  const bytes = decodeCanonicalBase64Url(value);
  if (!bytes) throw new Error("입장 응답을 확인하지 못했어요.");
  try { return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)); }
  catch { throw new Error("입장 응답을 확인하지 못했어요."); }
}
export function memberTargetRequest(record: MemberHandoff): MemberTargetRequest {
  const { server_id, registration_epoch, challenge_hash, handoff_state } = record;
  return { server_id, registration_epoch, challenge_hash, handoff_state };
}
export function createMemberHandoff(challenge: MemberChallenge, invite_token: string, meeting_id: string): MemberHandoff {
  return { ...challenge, invite_token, meeting_id,
    handoff_state: encodeBase64Url(crypto.getRandomValues(new Uint8Array(32))) };
}
export function storeMemberHandoff(record: MemberHandoff): void {
  purgeMemberHandoffs();
  sessionStorage.setItem(HOST_KEY + record.handoff_state, JSON.stringify(record));
}
export function purgeMemberHandoffs(): void {
  for (const key of Object.keys(sessionStorage)) {
    if (!key.startsWith(HOST_KEY)) continue;
    try { liveExpiry(JSON.parse(sessionStorage.getItem(key)!).expires_at); }
    catch { sessionStorage.removeItem(key); }
  }
}
export function centralMemberEntryUrl(centralOrigin: string, request: MemberTargetRequest): string {
  return `${exactCentralServerOrigin(centralOrigin)}/member-join#member-request=${encode(request)}`;
}
export function memberCallbackUrl(grant: MemberGrant, request: MemberTargetRequest): string {
  return `${exactCentralServerOrigin(grant.endpoint_origin)}/join#central-member=${encode({ ...request, grant })}`;
}
export function memberRetryUrl(origin: string, request: MemberTargetRequest): string {
  return `${exactCentralServerOrigin(origin)}/join#central-member=${encode({ ...request, retry: true })}`;
}

/** Called by main before React startup; even malformed grants leave browser history first. */
export function consumeMemberReturn(): MemberReturn | undefined {
  if (!window.location.hash.startsWith("#central-member")) return undefined;
  const fragment = window.location.hash;
  window.history.replaceState({}, "", window.location.pathname + window.location.search);
  let record: MemberHandoff | undefined;
  try {
    if (!fragment.startsWith("#central-member=")) throw new Error("입장 응답을 확인하지 못했어요.");
    const value = strictRecord(decode(fragment.slice("#central-member=".length)), "입장 응답");
    const { grant, retry, ...tuple } = value;
    const request = parseMemberTargetRequest(tuple);
    const key = HOST_KEY + request.handoff_state;
    const stored = sessionStorage.getItem(key);
    sessionStorage.removeItem(key);
    purgeMemberHandoffs();
    if (!stored) throw new Error("입장 기록이 없어요. 원래 초대 링크를 다시 열어 주세요.");
    const saved = strictRecord(JSON.parse(stored), "입장 기록");
    assertExactKeys(saved, ["server_id", "registration_epoch", "challenge_hash", "handoff_state",
      "challenge_id", "expires_at", "invite_token", "meeting_id"], "입장 기록");
    for (const field of ["challenge_id", "invite_token", "meeting_id"]) requiredString(saved, field, "입장 기록");
    record = saved as MemberHandoff;
    if (JSON.stringify(memberTargetRequest(record)) !== JSON.stringify(request)) {
      record = undefined;
      throw new Error("입장 기록이 일치하지 않아요. 원래 초대 링크를 다시 열어 주세요.");
    }
    liveExpiry(record.expires_at);
    if (retry === true && grant === undefined) return { record, retry: true };
    if (retry !== undefined) throw new Error("입장 응답을 확인하지 못했어요.");
    const parsed = parseMemberGrant(grant, request);
    if (parsed.endpoint_origin !== window.location.origin) throw new Error("서버 주소가 변경됐어요. 다시 시도해 주세요.");
    return { record, grant: parsed };
  } catch (error) {
    return { record, error: error instanceof Error && !(error instanceof SyntaxError) ? error.message : "입장 응답을 확인하지 못했어요." };
  }
}

export function consumeCentralMemberRequest(): MemberTargetRequest | undefined {
  if (window.location.pathname === "/member-join" && window.location.hash) {
    const fragment = window.location.hash;
    window.history.replaceState({}, "", "/member-join");
    if (!fragment.startsWith("#member-request=")) throw new Error("입장 요청을 확인하지 못했어요.");
    const request = parseMemberTargetRequest(decode(fragment.slice("#member-request=".length)));
    sessionStorage.setItem(CENTRAL_KEY, JSON.stringify({ request, expires: Date.now() + 300_000 }));
    return request;
  }
  const stored = sessionStorage.getItem(CENTRAL_KEY);
  if (!stored) return undefined;
  const pending = JSON.parse(stored);
  if (!Number.isSafeInteger(pending.expires) || pending.expires <= Date.now()) {
    sessionStorage.removeItem(CENTRAL_KEY);
    throw new Error("입장 요청이 만료됐어요. 초대 링크에서 다시 시도해 주세요.");
  }
  return parseMemberTargetRequest(pending.request);
}
export function clearCentralMemberRequest(): void { sessionStorage.removeItem(CENTRAL_KEY); }
