import { nativeGoogleAuthorization } from "./nativeGoogleAuthorization";
import { finishGoogleVerification } from "./googleRegistration";
import { clearCentralDirectoryCache, saveCentralDirectoryCache, type CentralServerDisplay } from "./directoryCache";
import { parseMemberGrant, type MemberTargetRequest } from "./memberConnect";
import { assertExactKeys, requiredString, strictRecord } from "../strictJsonContract";
import { exactCentralServerOrigin } from "./ownerConnect";
import { CentralTemporaryError, fetchCentral } from "./connectionError";
import {
  fetchDesktopCentralRegistration,
  fetchDesktopOperatorRuntime,
  isDesktopWebview,
} from "../desktopBridge";
import { encodeBase64Url } from "../base64Url";
import { type HostOs, type HostRegistrationEnvelope, validateHostName, validateHostOs, verifyCentralRegistrationEnvelope } from "./registrationProof";

const SESSION_KEY = "agentsassemble.centralSession.v1";
export const CENTRAL_SESSION_CHANGED_EVENT = "agentsassemble:central-session-changed";
export const CENTRAL_SESSION_CLEARED_EVENT = "agentsassemble:central-session-cleared";
const SERVERS_KEY = "agentsassemble.centralServers.v1";
const PENDING_RECOVERY_KEY = "agentsassemble.pendingRecoveryCode.v1";
const DB_NAME = "agentsassemble-central-identity-v1";
const STORE_NAME = "credentials";
const DEVICE_KEY = "device-v1";
const RECOVERY_CODE_PATTERN = /^(?:[ABCDEFGHJKLMNPQRSTUVWXYZ23456789]{4}-){7}[ABCDEFGHJKLMNPQRSTUVWXYZ23456789]{4}$/;

export type CentralPerson = {
  person_id: string;
  display_name: string;
  identity_kind: "guest" | "google";
  avatar_url?: string | null;
};

export type CentralSession = {
  token: string;
  expires_at: number;
  device_id: string;
  person: CentralPerson;
  pending_account_switch?: boolean;
};

export type CentralServer = {
  server_id: string;
  registration_epoch?: string;
  relation: "owner" | "bookmark" | "member";
  alias: string;
  default_name?: string;
  name_is_default?: boolean;
  /** Versioned reference to the server icon on the central origin; "" when unset. */
  icon?: string;
  host_os?: HostOs | null;
  host_public_key_jwk?: JsonWebKey;
  host_key_fingerprint: string;
  endpoint: null | {
    origin: string;
    generation: number;
    lease_expires_at?: number;
    status: "likely_online" | "published" | "offline";
    mode?: string;
    protocol?: string;
    account_deletion_protocol?: "v1" | null;
  };
};

export type CentralBootstrap = {
  person: CentralPerson;
  servers: CentralServer[];
  server_time: number;
  owner_server_conflict?: {
    servers: { server_id: string; registration_epoch: string; name: string; online: boolean; last_seen_at: number | null }[];
    resolution: { keeper_server_id: string; keeper_registration_epoch: string; revision: string } | null;
  } | null;
};

export type CentralConnectGrant = {
  grant_token: string;
  server_id: string;
  origin: string;
  generation: number;
  expires_at: number;
};

export type CentralGuestResult = {
  person: CentralPerson;
  session: Omit<CentralSession, "person">;
  recovery_code: string;
  previous_code_revoked?: boolean;
};

export type CentralGoogleHandoff = {
  handoff_id: string;
  authorization_url: string;
  state: string;
  expires_at: number;
};

type StoredDevice = {
  deviceId: string;
  privateKey: CryptoKey;
  publicJwk: JsonWebKey;
};

class CentralAuthError extends Error {
  constructor(message: string, readonly code?: string) { super(message); }
}

let devicePromise: Promise<StoredDevice> | undefined;

function configuredUrl(): string {
  const raw = String(
    import.meta.env.VITE_AGENTSASSEMBLE_CENTRAL_URL || ""
  )
    .trim()
    .replace(/\/+$/, "");
  if (!raw) return "";
  try {
    const parsed = new URL(raw);
    const loopback = ["localhost", "127.0.0.1", "::1"].includes(parsed.hostname);
    if (
      parsed.username ||
      parsed.password ||
      parsed.search ||
      parsed.hash ||
      parsed.pathname !== "/"
    ) {
      return "";
    }
    if (parsed.protocol !== "https:" && !(parsed.protocol === "http:" && loopback)) return "";
    return parsed.origin;
  } catch {
    return "";
  }
}

export function centralAccountEntryUrl(): string {
  return configuredUrl() ? `${configuredUrl()}/` : "";
}

export function isCentralWebEntry(): boolean {
  return Boolean(configuredUrl()) && window.location.origin === configuredUrl() && !isDesktopWebview();
}

export function centralIdentityConfigured(): boolean {
  return Boolean(configuredUrl());
}

function randomUrlToken(bytes = 18): string {
  const value = new Uint8Array(bytes);
  crypto.getRandomValues(value);
  return encodeBase64Url(value);
}

async function sha256(value: string): Promise<string> {
  return encodeBase64Url(
    await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value))
  );
}

function openCredentialDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(STORE_NAME)) {
        request.result.createObjectStore(STORE_NAME);
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () =>
      reject(request.error || new Error("기기 보안 저장소를 열지 못했어요."));
  });
}

async function loadOrCreateDevice(): Promise<StoredDevice> {
  const db = await openCredentialDb();
  try {
    // Explicit logout starts a new account slot. Older logout builds left this
    // marker with the previous account's permanent registration, so repair that
    // transition before login as well. Keep login blocked across a crash.
    if (localStorage.getItem(SESSION_KEY) === "logged-out") {
      await new Promise<void>((resolve, reject) => {
        const transaction = db.transaction(STORE_NAME, "readwrite");
        transaction.objectStore(STORE_NAME).delete(DEVICE_KEY);
        transaction.oncomplete = () => resolve();
        transaction.onerror = transaction.onabort = () => reject(transaction.error || new Error("로그아웃한 기기 등록을 초기화하지 못했어요."));
      });
      localStorage.setItem(SESSION_KEY, "logged-out:fresh-device");
    }
    const existing = await new Promise<StoredDevice | undefined>((resolve, reject) => {
      const request = db
        .transaction(STORE_NAME, "readonly")
        .objectStore(STORE_NAME)
        .get(DEVICE_KEY);
      request.onsuccess = () => resolve(request.result as StoredDevice | undefined);
      request.onerror = () => reject(request.error);
    });
    if (existing?.privateKey && existing.deviceId && existing.publicJwk) return existing;

    const generated = await crypto.subtle.generateKey(
      { name: "ECDSA", namedCurve: "P-256" },
      true,
      ["sign", "verify"]
    );
    const publicJwk = await crypto.subtle.exportKey("jwk", generated.publicKey);
    const privateJwk = await crypto.subtle.exportKey("jwk", generated.privateKey);
    const privateKey = await crypto.subtle.importKey(
      "jwk",
      privateJwk,
      { name: "ECDSA", namedCurve: "P-256" },
      false,
      ["sign"]
    );
    const created: StoredDevice = {
      deviceId: `dev_${randomUrlToken(18)}`,
      privateKey,
      publicJwk,
    };
    await new Promise<void>((resolve, reject) => {
      const transaction = db.transaction(STORE_NAME, "readwrite");
      transaction.objectStore(STORE_NAME).put(created, DEVICE_KEY);
      transaction.oncomplete = () => resolve();
      transaction.onerror = transaction.onabort = () => reject(transaction.error || new Error("기기 로그인 키를 저장하지 못했어요."));
    });
    return created;
  } finally {
    db.close();
  }
}

function storedDevice(): Promise<StoredDevice> {
  if (!devicePromise) {
    devicePromise = loadOrCreateDevice().catch((error) => {
      devicePromise = undefined;
      throw error;
    });
  }
  return devicePromise;
}

export function saveSession(
  result:
    | CentralGuestResult
    | { person: CentralPerson; session: Omit<CentralSession, "person"> }
): CentralSession {
  const previous = loadCentralSession();
  const session: CentralSession = { ...result.session, person: result.person,
    ...(centralSessionLoggedOut() || (previous?.person.person_id === result.person.person_id && previous.pending_account_switch)
      ? { pending_account_switch: true } : {}),
  };
  if (previous?.person.person_id !== result.person.person_id) clearCentralDirectoryCache();
  localStorage.setItem(SESSION_KEY, JSON.stringify(session));
  window.dispatchEvent(new Event(CENTRAL_SESSION_CHANGED_EVENT));
  return session;
}

export function saveGuestResult(result: CentralGuestResult): CentralGuestResult {
  const recoveryCode = String(result.recovery_code || "").trim().toUpperCase();
  if (!RECOVERY_CODE_PATTERN.test(recoveryCode)) {
    throw new Error("로그인 서버가 올바른 형식의 복구 코드를 반환하지 않았습니다.");
  }
  // Keep the sole plaintext copy only on this client until the user confirms
  // saving it. A restart must return to the warning screen instead of silently
  // entering the application with an unacknowledged recovery secret.
  localStorage.setItem(PENDING_RECOVERY_KEY, recoveryCode);
  saveSession(result);
  return result;
}

export function loadPendingCentralRecoveryCode(): string {
  try {
    const value = String(localStorage.getItem(PENDING_RECOVERY_KEY) || "")
      .trim()
      .toUpperCase();
    if (!RECOVERY_CODE_PATTERN.test(value)) {
      localStorage.removeItem(PENDING_RECOVERY_KEY);
      return "";
    }
    return value;
  } catch {
    return "";
  }
}

export function clearPendingCentralRecoveryCode(): void {
  localStorage.removeItem(PENDING_RECOVERY_KEY);
}

export function loadCentralSession(): CentralSession | null {
  // The legacy directory has no account owner and contains connection material.
  localStorage.removeItem(SERVERS_KEY);
  try {
    const parsed = JSON.parse(
      localStorage.getItem(SESSION_KEY) || "null"
    ) as CentralSession | null;
    if (!parsed?.token || !parsed.device_id || !parsed.person?.person_id) return null;
    if (parsed.expires_at <= Math.floor(Date.now() / 1000)) {
      clearCentralSession();
      return null;
    }
    return parsed;
  } catch {
    return null;
  }
}

export function clearCentralSession(): void {
  if (!centralSessionLoggedOut()) {
    localStorage.setItem(SESSION_KEY, "logged-out");
    devicePromise = undefined;
  }
  localStorage.removeItem(SERVERS_KEY);
  clearCentralDirectoryCache();
  window.dispatchEvent(new Event(CENTRAL_SESSION_CLEARED_EVENT));
}

export function centralSessionLoggedOut(): boolean {
  return ["logged-out", "logged-out:fresh-device"].includes(localStorage.getItem(SESSION_KEY) || "");
}

export async function logoutCentral(): Promise<void> {
  const session = loadCentralSession();
  if (session) {
    try {
      await signedRequest(session, "/v1/logout", "POST");
    } catch (error) {
      if (!(error instanceof CentralAuthError) || error.code !== "invalid_session") throw error;
    }
  }
  clearCentralSession();
  clearPendingCentralRecoveryCode();
  localStorage.setItem(SESSION_KEY, "logged-out");
  devicePromise = undefined;
}

async function responsePayload<T>(response: Response, central = true, signal?: AbortSignal): Promise<T> {
  const payload = (await response.json().catch((error) => {
    if (response.ok) {
      if (signal?.aborted && signal.reason?.name !== "TimeoutError") throw error;
      if (central && (signal?.reason?.name === "TimeoutError" || error instanceof TypeError || error instanceof DOMException && error.name === "TimeoutError")) {
        throw new CentralTemporaryError("로그인 서버 응답을 받지 못했어요.");
      }
      throw error;
    }
    return {};
  })) as {
    code?: string;
    error?: { code?: string; message?: string; server_id?: string; registration_epoch?: string };
  } & T;
  if (!response.ok) {
    const code = central ? payload?.error?.code : payload?.code;
    const message =
      (response.status === 409 && code === "server_move_unsupported" ? "서버 옮기기는 아직 지원하지 않아요"
        : code === "duplicate_resolution_conflict" ? "다른 기기에서 서버를 선택했어요. 목록을 다시 확인해 주세요."
        : central && code === "actor_quota_exhausted" ? "오늘 계정·기기·서버의 변경 요청 한도를 다 썼어요. UTC 자정(한국 시간 오전 9시) 이후 다시 시도해 주세요."
        : code === "server_retirement_capacity" ? "잠시 후 다시 시도해 주세요" : undefined) ||
      (central ? payload?.error?.message : typeof payload?.error === "string" ? payload.error : undefined) ||
      `로그인 서버가 HTTP ${response.status}을 반환했습니다.`;
    if (response.status === 401) throw new CentralAuthError(message, code);
    if (central && (response.status === 429 || response.status >= 500)) throw new CentralTemporaryError(message);
    throw Object.assign(new Error(message), { status: response.status, code,
      server_id: payload?.error?.server_id, registration_epoch: payload?.error?.registration_epoch });
  }
  return payload;
}

export async function unsignedPost<T>(
  path: string,
  body: Record<string, unknown>,
  signal?: AbortSignal
): Promise<T> {
  localStorage.removeItem(SERVERS_KEY);
  const response = await fetch(`${configuredUrl()}${path}`, {
    method: "POST",
    mode: "cors",
    cache: "no-store",
    credentials: "omit",
    redirect: "error",
    referrerPolicy: "no-referrer",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    signal,
  });
  return responsePayload<T>(response);
}

function fetchLocalRuntime(path: string, init: RequestInit = {}): Promise<Response> {
  localStorage.removeItem(SERVERS_KEY);
  return isDesktopWebview()
    ? fetchDesktopOperatorRuntime(path, init)
    : fetch(path, init);
}

export function parseCentralGoogleHandoff(value: unknown): CentralGoogleHandoff {
  const handoff = value as Partial<CentralGoogleHandoff> | null;
  if (!handoff || typeof handoff !== "object") {
    throw new Error("로그인 서버가 올바르지 않은 응답을 반환했습니다.");
  }
  const state = String(handoff.state || "").trim();
  if (!state) {
    throw new Error(
      "로그인 서버가 현재 앱보다 오래된 버전입니다. Worker 업데이트가 필요합니다."
    );
  }
  const result: CentralGoogleHandoff = {
    handoff_id: String(handoff.handoff_id || "").trim(),
    authorization_url: String(handoff.authorization_url || "").trim(),
    state,
    expires_at: Number(handoff.expires_at || 0),
  };
  let authorizationUrl: URL;
  try {
    authorizationUrl = new URL(result.authorization_url);
  } catch {
    throw new Error("로그인 서버가 올바르지 않은 응답을 반환했습니다.");
  }
  if (
    !result.handoff_id ||
    result.state.length < 32 ||
    result.state.length > 128 ||
    !/^[A-Za-z0-9._:-]+$/.test(result.state) ||
    !Number.isSafeInteger(result.expires_at) ||
    result.expires_at <= Math.floor(Date.now() / 1000) ||
    authorizationUrl.protocol !== "https:" ||
    authorizationUrl.hostname !== "accounts.google.com" ||
    authorizationUrl.pathname !== "/o/oauth2/v2/auth" ||
    authorizationUrl.username ||
    authorizationUrl.password ||
    authorizationUrl.hash ||
    authorizationUrl.searchParams.get("response_type") !== "code" ||
    authorizationUrl.searchParams.get("scope") !== "openid profile" ||
    authorizationUrl.searchParams.get("state") !== result.state ||
    authorizationUrl.searchParams.get("code_challenge_method") !== "S256" ||
    !authorizationUrl.searchParams.get("client_id") ||
    !authorizationUrl.searchParams.get("nonce")
  ) {
    throw new Error("로그인 서버가 올바르지 않은 응답을 반환했습니다.");
  }
  return result;
}

export async function signedRequest<T>(
  session: CentralSession,
  path: string,
  method: "GET" | "POST" | "DELETE",
  bodyValue?: Record<string, unknown>,
  signal?: AbortSignal
): Promise<T> {
  return responsePayload<T>(await signedFetch(session, path, method, bodyValue, signal), true, signal);
}

async function signedFetch(
  session: CentralSession,
  path: string,
  method: "GET" | "POST" | "DELETE",
  bodyValue?: Record<string, unknown>,
  signal?: AbortSignal
): Promise<Response> {
  const device = await storedDevice();
  if (device.deviceId !== session.device_id) {
    if (loadCentralSession()?.token === session.token) clearCentralSession();
    throw new CentralAuthError(
      "이 기기의 로그인 키가 바뀌었어요. 다시 로그인해 주세요."
    );
  }
  const body = bodyValue ? JSON.stringify(bodyValue) : "";
  const timestamp = Math.floor(Date.now() / 1000);
  const nonce = randomUrlToken(18);
  const canonical = [
    "AA-DEVICE-1",
    method,
    path,
    String(timestamp),
    nonce,
    await sha256(body),
    await sha256(session.token),
    device.deviceId,
  ].join("\n");
  const signature = await crypto.subtle.sign(
    { name: "ECDSA", hash: "SHA-256" },
    device.privateKey,
    new TextEncoder().encode(canonical)
  );
  const response = await fetchCentral(`${configuredUrl()}${path}`, {
    method,
    mode: "cors",
    cache: "no-store",
    credentials: "omit",
    redirect: "error",
    referrerPolicy: "no-referrer",
    headers: {
      authorization: `Bearer ${session.token}`,
      "content-type": "application/json",
      "x-aa-admission-protocol": "secure_admission_v1",
      "x-aa-device-id": device.deviceId,
      "x-aa-timestamp": String(timestamp),
      "x-aa-nonce": nonce,
      "x-aa-signature": encodeBase64Url(signature),
    },
    body: body || undefined,
    signal,
  });
  if (response.status === 401 && loadCentralSession()?.token === session.token) clearCentralSession();
  return response;
}

export async function authDeviceBody(displayName?: string): Promise<Record<string, unknown>> {
  const device = await storedDevice();
  return {
    device_id: device.deviceId,
    device_public_key_jwk: device.publicJwk,
    device_label: "AgentsAssemble device",
    ...(displayName ? { display_name: displayName } : {}),
  };
}

export async function createCentralGuest(displayName: string): Promise<CentralGuestResult> {
  const result = await unsignedPost<CentralGuestResult>(
    "/v1/auth/guest",
    await authDeviceBody(displayName)
  );
  return saveGuestResult(result);
}

export async function recoverCentralGuest(
  recoveryCode: string
): Promise<CentralGuestResult> {
  const result = await unsignedPost<CentralGuestResult>("/v1/auth/recover", {
    ...(await authDeviceBody()),
    recovery_code: recoveryCode,
  });
  return saveGuestResult(result);
}

export async function loginCentralGoogle(status?: (message: string) => void, signal?: AbortSignal): Promise<CentralSession> {
  localStorage.removeItem(SERVERS_KEY);
  const expectedToken = loadCentralSession()?.token ?? null;
  const {body, expiresAt} = await nativeGoogleAuthorization(async request => unsignedPost<unknown>(
    "/v1/auth/google/native/verify-start", {...await authDeviceBody(), ...request}, signal
  ),status,signal);
  const result=await unsignedPost<unknown>("/v1/auth/google/native/verify-complete",body,signal);
  return finishGoogleVerification(result,"native",body,expiresAt,expectedToken);
}

export async function bootstrapCentral(signal?: AbortSignal): Promise<CentralBootstrap | null> {
  const session = loadCentralSession();
  if (!session) return null;
  const payload = await signedRequest<CentralBootstrap>(
    session,
    "/v1/bootstrap",
    "GET", undefined, signal ? AbortSignal.any([signal, AbortSignal.timeout(10_000)]) : AbortSignal.timeout(10_000)
  );
  signal?.throwIfAborted();
  if (!payload.person || payload.person.person_id !== session.person.person_id || !Array.isArray(payload.servers)) throw new Error("서버 목록 응답이 올바르지 않습니다.");
  payload.servers = payload.servers.map(server => server.relation === "member" ? {
    server_id: server.server_id, relation: "member", alias: server.alias, icon: server.icon,
    registration_epoch: server.registration_epoch, host_key_fingerprint: server.host_key_fingerprint,
    host_public_key_jwk: server.host_public_key_jwk,
    endpoint: server.endpoint ? { origin: server.endpoint.origin, generation: server.endpoint.generation,
      status: server.endpoint.status, mode: server.endpoint.mode, protocol: server.endpoint.protocol, account_deletion_protocol: server.endpoint.account_deletion_protocol } : null,
  } : server);
  const current = loadCentralSession();
  if (current?.token !== session.token) throw new CentralAuthError("로그인 상태가 바뀌었어요. 다시 로그인해 주세요.");
  localStorage.setItem(SESSION_KEY, JSON.stringify({ ...current, person: payload.person }));
  saveCentralDirectoryCache(payload.person.person_id, payload.servers);
  return payload;
}

export async function issueCentralMemberGrant(request: MemberTargetRequest, signal?: AbortSignal) {
  const session = loadCentralSession();
  if (!session) throw new CentralAuthError("로그인이 필요해요.");
  const value = await signedRequest<unknown>(session,
    `/v1/servers/${encodeURIComponent(request.server_id)}/${request.purpose === "connect" ? "member-connect-grants" : "member-grants"}`, "POST",
    { registration_epoch: request.registration_epoch, challenge_hash: request.challenge_hash, purpose: request.purpose || "admission" }, signal);
  if (loadCentralSession()?.token !== session.token) throw new CentralAuthError("로그인 계정이 바뀌었어요. 다시 시도해 주세요.");
  return parseMemberGrant(value, request);
}

export async function previewCentralMember(request: MemberTargetRequest, signal?: AbortSignal) {
  const session = loadCentralSession();
  if (!session) throw new CentralAuthError("로그인이 필요해요.");
  const value = strictRecord(await signedRequest<unknown>(session,
    `/v1/servers/${encodeURIComponent(request.server_id)}/member-preview`, "POST",
    { registration_epoch: request.registration_epoch }, signal), "입장할 서버");
  assertExactKeys(value, ["server_id", "label", "endpoint_origin", "endpoint_generation"], "입장할 서버");
  if (value.server_id !== request.server_id || typeof value.endpoint_generation !== "number" ||
      !Number.isSafeInteger(value.endpoint_generation) || value.endpoint_generation < 1) {
    throw new Error("입장할 서버 정보를 확인하지 못했어요.");
  }
  if (loadCentralSession()?.token !== session.token) throw new CentralAuthError("로그인 계정이 바뀌었어요. 다시 시도해 주세요.");
  return { server_id: request.server_id, label: requiredString(value, "label", "입장할 서버"),
    endpoint_origin: exactCentralServerOrigin(requiredString(value, "endpoint_origin", "입장할 서버")),
    endpoint_generation: value.endpoint_generation };
}

export { openCentralOwnedServer } from "./secureOwnerEntry";

export type LocalServerInfo = {
  server_id: string;
  host_public_key_jwk: JsonWebKey;
  host_key_fingerprint: string;
  protocol_version: number;
  status: string;
};

export async function fetchLocalServerInfo(): Promise<LocalServerInfo> {
  const response = await fetchLocalRuntime("/api/server-info", {
    cache: "no-store",
  });
  return responsePayload<LocalServerInfo>(response, false);
}

async function updateLocalRegistrationEpoch(serverId: string, epoch: string | null, deviceToken = ""): Promise<void> {
  const request = { method: "POST", cache: "no-store",
    headers: { "content-type": "application/json", ...(isDesktopWebview() ? {} : { "x-device-token": deviceToken }) },
    body: JSON.stringify({ server_id: serverId, registration_epoch: epoch }),
  } satisfies RequestInit;
  const response = isDesktopWebview() ? (await fetchDesktopCentralRegistration(request)).response
    : await fetch("/api/central-directory/registration-proof", request);
  await responsePayload(response, false);
}

export async function resolveCentralServerDuplicates(conflict: NonNullable<CentralBootstrap["owner_server_conflict"]>, keeperId: string, local?: { serverId: string; deviceToken: string }): Promise<void> {
  const session = loadCentralSession();
  if (!session) throw new CentralAuthError("로그인이 필요해요. 다시 로그인해 주세요.");
  const keeper = conflict.servers.find(server => server.server_id === keeperId);
  if (!keeper) throw new Error("남길 서버를 다시 선택해 주세요.");
  let revision = conflict.resolution?.revision || null;
  for (const server of conflict.servers.filter(server => server.server_id !== keeperId)) {
    const result = await signedRequest<{ status: string; server_id: string; registration_epoch: string; resolution: { revision: string } | null }>(session, "/v1/servers/resolve-duplicates", "POST", {
      keeper_server_id: keeper.server_id, keeper_registration_epoch: keeper.registration_epoch,
      server_id: server.server_id, registration_epoch: server.registration_epoch, expected_revision: revision,
    });
    if (result.status !== "server_retired" || result.server_id !== server.server_id || result.registration_epoch !== server.registration_epoch) throw new Error("서버 은퇴 결과를 확인하지 못했어요. 목록을 다시 확인해 주세요.");
    if (local?.serverId === server.server_id) {
      localStorage.setItem(PENDING_DEMOTION_KEY, JSON.stringify({ server_id: server.server_id, registration_epoch: server.registration_epoch }));
      await retryPendingLocalDemotion(local.deviceToken);
    }
    revision = result.resolution?.revision || null;
    if (loadCentralSession()?.token !== session.token) throw new CentralAuthError("로그인 계정이 바뀌었어요. 다시 확인해 주세요.");
  }
}

const PENDING_DEMOTION_KEY = "agentsassemble.pendingLocalDemotion.v1";

export function hasPendingLocalDemotion(): boolean {
  return localStorage.getItem(PENDING_DEMOTION_KEY) !== null;
}

export async function retryPendingLocalDemotion(deviceToken: string): Promise<void> {
  const pending = localStorage.getItem(PENDING_DEMOTION_KEY);
  if (!pending) return;
  const value = JSON.parse(pending) as { server_id: string; registration_epoch: string | null };
  const state = await localHostingState(value.server_id, deviceToken, "retired", value.registration_epoch);
  if (state !== "retired") throw new Error("이 컴퓨터의 은퇴를 확인하지 못했어요. 다시 시도해 주세요.");
  if (localStorage.getItem(PENDING_DEMOTION_KEY) === pending) localStorage.removeItem(PENDING_DEMOTION_KEY);
}

export async function localHostingState(serverId: string, deviceToken: string, state = "status", epoch: string | null = null): Promise<"device" | "retired" | "account_deleted" | null> {
  if (state === "status") await retryPendingLocalDemotion(deviceToken);
  const request = { method: "POST", cache: "no-store",
    headers: { "content-type": "application/json", ...(isDesktopWebview() ? {} : { "x-device-token": deviceToken }) },
    body: JSON.stringify({ server_id: serverId, hosting_state: state, ...(state === "retired" ? { registration_epoch: epoch } : {}) }),
  } satisfies RequestInit;
  const response = isDesktopWebview() ? (await fetchDesktopCentralRegistration(request)).response
    : await fetch("/api/central-directory/registration-proof", request);
  const payload = await responsePayload<{ hosting_state: "device" | "retired" | "account_deleted" | null }>(response, false);
  if (payload.hosting_state !== null && payload.hosting_state !== "device" && payload.hosting_state !== "retired" && payload.hosting_state !== "account_deleted") throw new Error("이 컴퓨터의 서버 상태를 확인하지 못했어요.");
  return payload.hosting_state;
}

export async function registerLocalServer(deviceToken: string, name?: string): Promise<void> {
  const session = loadCentralSession();
  if (!session) throw new CentralAuthError("로그인이 필요해요. 다시 로그인해 주세요.");
  for (let attempt = 0; attempt < 2; attempt++) {
    const registrationRequest = {
      method: "POST", cache: "no-store",
      headers: { "content-type": "application/json",
        ...(isDesktopWebview() ? {} : { "x-device-token": deviceToken }) },
      body: JSON.stringify({ owner_person_id: session.person.person_id,
        ...(session.pending_account_switch ? { claim_ownership: true } : {}) }),
    } satisfies RequestInit;
    const registration = isDesktopWebview()
      ? await fetchDesktopCentralRegistration(registrationRequest) : null;
    const proofResponse = registration ? registration.response
      : await fetch("/api/central-directory/registration-proof", registrationRequest);
    const payload = await responsePayload<HostRegistrationEnvelope>(proofResponse, false);
    const local = registration ? await verifyCentralRegistrationEnvelope(payload,
      session.person.person_id, registration.binding, session.pending_account_switch === true) : payload;
    validateHostName(local.host_name);
    validateHostOs(local.host_os);
    let registered: { registration_epoch?: string };
    try {
      registered = await signedRequest(session, "/v1/servers", "POST", {
        server_id: local.server_id, label: local.host_name, host_os: local.host_os, name_revision: local.name_revision,
        host_public_key_jwk: local.host_public_key_jwk,
        host_registration_proof: local.host_registration_proof,
        registration_epoch: local.registration_epoch,
        ...(session.pending_account_switch ? { claim_ownership: true } : {}),
      });
    } catch (error) {
      if (error instanceof Error && "status" in error && "code" in error &&
        (error.status === 410 && "server_id" in error && error.server_id === local.server_id &&
           "registration_epoch" in error && typeof error.registration_epoch === "string" && error.registration_epoch.length > 0 &&
           (!local.registration_epoch || error.registration_epoch === local.registration_epoch) &&
           (error.code === "server_retired" || error.code === "registration_absent" && local.registration_epoch))) {
        localStorage.setItem(PENDING_DEMOTION_KEY, JSON.stringify({ server_id: local.server_id, registration_epoch: local.registration_epoch || null }));
        await retryPendingLocalDemotion(deviceToken);
        throw error;
      }
      if (attempt !== 0 || session.pending_account_switch || !(error instanceof Error) ||
        !("status" in error) || error.status !== 409 || !("code" in error) || error.code !== "incarnation_conflict") throw error;
      await updateLocalRegistrationEpoch(local.server_id, null, deviceToken);
      continue;
    }
    if (registered.registration_epoch !== undefined) {
      await updateLocalRegistrationEpoch(local.server_id, registered.registration_epoch, deviceToken);
    }
    if (name !== undefined) {
      const current = await bootstrapCentral();
      const server = current?.servers.find(item => item.server_id === local.server_id);
      if (!server) throw new Error("등록한 서버를 확인하지 못했어요. 목록을 새로고침해 주세요.");
      await renameCentralServer(server, name);
    }
    break;
  }
  const current = loadCentralSession();
  if (current?.token === session.token && current.pending_account_switch) {
    delete current.pending_account_switch;
    localStorage.setItem(SESSION_KEY, JSON.stringify(current));
  }
}

export async function waitForLocalDirectory(): Promise<void> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), 8000);
  try {
    const response = await fetchLocalRuntime("/api/rooms", {
      cache: "no-store",
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
  } finally {
    window.clearTimeout(timer);
  }
}

export function isCentralAuthenticationError(error: unknown): boolean {
  return error instanceof CentralAuthError;
}

export async function renameCentralServer(server: CentralServerDisplay, name: string | null): Promise<void> {
  const session = loadCentralSession();
  if (!session) throw new CentralAuthError("로그인이 필요해요. 다시 로그인해 주세요.");
  if (server.relation !== "owner") throw new Error("서버 소유자만 이름을 바꿀 수 있어요.");
  if (!server.registration_epoch?.trim()) throw new Error("서버 등록 정보를 확인할 수 없어요. 목록을 새로고침해 주세요.");
  await signedRequest(session, `/v1/servers/${encodeURIComponent(server.server_id)}/name`, "POST", {
    registration_epoch: server.registration_epoch,
    ...(name === null ? { reset_default: true } : { name }),
    expected_name: server.alias || server.server_id,
    expected_name_is_default: server.name_is_default,
  });
  if (loadCentralSession()?.token !== session.token) throw new CentralAuthError("로그인 계정이 바뀌었어요. 다시 확인해 주세요.");
}

const SERVER_ICON_REFERENCE = /^\/v1\/servers\/[^/?#]+\/icon\/[^/?#]+\.png$/;

/**
 * Server icons are served only to signed central requests, so the image is fetched
 * here and shown through a local object URL. Only the directory's own relative
 * reference is accepted, so credentials never go to another origin.
 */
export async function fetchCentralServerIcon(reference: string): Promise<Blob> {
  const session = loadCentralSession();
  if (!session) throw new CentralAuthError("로그인이 필요해요. 다시 로그인해 주세요.");
  if (!SERVER_ICON_REFERENCE.test(reference)) throw new Error("서버 아이콘 주소가 올바르지 않습니다.");
  const response = await signedFetch(session, reference, "GET");
  if (!response.ok) await responsePayload<never>(response);
  return response.blob();
}

async function pngDataUrl(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  }
  return `data:image/png;base64,${btoa(binary)}`;
}

/** Sets (512x512 PNG) or removes (null) a server icon; returns the new reference. */
export async function setCentralServerIcon(server: CentralServerDisplay, icon: File | null): Promise<string> {
  const session = loadCentralSession();
  if (!session) throw new CentralAuthError("로그인이 필요해요. 다시 로그인해 주세요.");
  if (server.relation !== "owner") throw new Error("서버 소유자만 아이콘을 바꿀 수 있어요.");
  const result = await signedRequest<{ icon: string }>(
    session,
    `/v1/servers/${encodeURIComponent(server.server_id)}/icon`,
    "POST",
    { registration_epoch: server.registration_epoch, icon: icon ? await pngDataUrl(icon) : "", expected_icon: server.icon || "" }
  );
  if (loadCentralSession()?.token !== session.token) throw new CentralAuthError("로그인 계정이 바뀌었어요. 다시 확인해 주세요.");
  return String(result.icon || "");
}

export async function setCentralMemberHidden(server: {server_id:string;registration_epoch?:string}, hidden: boolean): Promise<void> {
  const session=loadCentralSession();
  if (!session || !server.registration_epoch) throw new CentralAuthError("서버 등록 정보를 확인하지 못했어요. 다시 로그인해 주세요.");
  await signedRequest(session, `/v1/member-servers/${encodeURIComponent(server.server_id)}/${hidden ? "hide" : "unhide"}`, "POST", {registration_epoch:server.registration_epoch});
  if (loadCentralSession()?.token !== session.token) throw new CentralAuthError("로그인 계정이 바뀌었어요. 다시 시도해 주세요.");
}
export async function openCentralMemberServer(server: CentralServer): Promise<void> {
  if (server.relation !== "member" || !server.registration_epoch || server.endpoint?.mode !== "event_secure_v1") throw new Error("이 서버에 연결할 수 없어요. 서버 앱을 업데이트해 주세요.");
  const { selectRemoteMember } = await import("../remote/remoteWorkspace");
  selectRemoteMember({ server_id: server.server_id, registration_epoch: server.registration_epoch });
}
