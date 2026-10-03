import { ApiError } from "./apiErrors";
import { parseStrictRoomDirectory, parseStrictRoomCreateResponse } from "./roomDirectoryContract";
import { decodeCanonicalBase64Url, encodeBase64Url } from "./base64Url";
import { parseOperatorPairingRedeemResponse } from "./roomAdmissionContract";
import {
  assertExactKeys,
  requiredString,
  strictRecord,
} from "./strictJsonContract";

const FRAGMENT_PREFIX = "central-owner=";
const GRANT_PATTERN = /^aacg1\.[A-Za-z0-9_-]{43}$/;
const ID_PATTERN = /^[A-Za-z0-9._:-]{1,200}$/;

export type CentralOwnerConnect = {
  grantToken: string;
  serverId: string;
  generation: number;
  expiresAt: number;
  hostPublicKeyX: string;
  hostKeyFingerprint: string;
};


function normalize(value: unknown): CentralOwnerConnect | null {
  try {
    const record = strictRecord(value, "중앙 서버 접속권");
    assertExactKeys(
      record,
      [
        "grantToken",
        "serverId",
        "generation",
        "expiresAt",
        "hostPublicKeyX",
        "hostKeyFingerprint",
      ],
      "중앙 서버 접속권"
    );
    const result: CentralOwnerConnect = {
      grantToken: requiredString(record, "grantToken", "중앙 서버 접속권"),
      serverId: requiredString(record, "serverId", "중앙 서버 접속권"),
      generation: Number(record.generation),
      expiresAt: Number(record.expiresAt),
      hostPublicKeyX: requiredString(record, "hostPublicKeyX", "중앙 서버 접속권"),
      hostKeyFingerprint: requiredString(
        record,
        "hostKeyFingerprint",
        "중앙 서버 접속권"
      ),
    };
    if (
      !GRANT_PATTERN.test(result.grantToken) ||
      !ID_PATTERN.test(result.serverId) ||
      !Number.isSafeInteger(result.generation) ||
      result.generation < 1 ||
      !Number.isSafeInteger(result.expiresAt) ||
      result.expiresAt <= Math.floor(Date.now() / 1000) ||
      !/^[A-Za-z0-9_-]{43}$/.test(result.hostPublicKeyX) ||
      !/^[A-Za-z0-9_-]{43}$/.test(result.hostKeyFingerprint)
    ) {
      return null;
    }
    return result;
  } catch {
    return null;
  }
}

export function centralOwnerServerUrl(
  origin: string,
  connect: CentralOwnerConnect
): string {
  const url = new URL(origin);
  if (
    url.protocol !== "https:" ||
    url.origin !== origin ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  ) {
    throw new Error("중앙 서버 주소가 안전하지 않습니다.");
  }
  const encoded = encodeBase64Url(
    new TextEncoder().encode(JSON.stringify(connect))
  );
  // Remote owners enter through the existing public device-pairing shell.
  // /app is intentionally private; authority still comes from the signed grant.
  url.pathname = "/pair";
  url.hash = `${FRAGMENT_PREFIX}${encoded}`;
  return url.toString();
}

export function centralOwnerConnectFromUrl(url: string): CentralOwnerConnect | null {
  try {
    const parsed = new URL(url);
    const fragment = parsed.hash.slice(1);
    if (!fragment.startsWith(FRAGMENT_PREFIX)) return null;
    const bytes = decodeCanonicalBase64Url(fragment.slice(FRAGMENT_PREFIX.length));
    if (!bytes || bytes.length > 12_000) return null;
    return normalize(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)));
  } catch {
    return null;
  }
}

export function consumeCentralOwnerConnectFromUrl(): CentralOwnerConnect | null {
  const result = centralOwnerConnectFromUrl(window.location.href);
  if (result) {
    window.history.replaceState({}, "", `${window.location.pathname}${window.location.search}`);
  }
  return result;
}

async function responseJson(response: Response, label: string): Promise<unknown> {
  const payload = await response.json().catch(() => null);
  if (!response.ok) throw new Error(`${label}에 실패했습니다. HTTP ${response.status}`);
  return payload;
}

export async function verifyCentralOwnerHost(
  connect: CentralOwnerConnect
): Promise<void> {
  const challenge = encodeBase64Url(crypto.getRandomValues(new Uint8Array(32)));
  const response = await fetch("/api/server-info/challenge", {
    method: "POST",
    cache: "no-store",
    credentials: "omit",
    redirect: "error",
    referrerPolicy: "no-referrer",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ challenge }),
  });
  const record = strictRecord(
    await responseJson(response, "서버 신원 확인"),
    "서버 신원 확인"
  );
  assertExactKeys(
    record,
    [
      "server_id",
      "origin",
      "host_public_key_jwk",
      "host_key_fingerprint",
      "protocol_version",
      "challenge",
      "issued_at",
      "signature",
    ],
    "서버 신원 확인"
  );
  const jwk = strictRecord(record.host_public_key_jwk, "서버 공개 키");
  assertExactKeys(jwk, ["crv", "ext", "key_ops", "kty", "x"], "서버 공개 키");
  const issuedAt = Number(record.issued_at);
  const now = Math.floor(Date.now() / 1000);
  if (
    record.server_id !== connect.serverId ||
    record.origin !== window.location.origin ||
    record.host_key_fingerprint !== connect.hostKeyFingerprint ||
    record.challenge !== challenge ||
    record.protocol_version !== 1 ||
    !Number.isSafeInteger(issuedAt) ||
    Math.abs(now - issuedAt) > 60 ||
    jwk.crv !== "Ed25519" ||
    jwk.ext !== true ||
    jwk.kty !== "OKP" ||
    !Array.isArray(jwk.key_ops) ||
    jwk.key_ops.length !== 1 ||
    jwk.key_ops[0] !== "verify" ||
    jwk.x !== connect.hostPublicKeyX
  ) {
    throw new Error("선택한 중앙 서버와 열린 서버의 신원이 다릅니다.");
  }
  const canonicalJwk = JSON.stringify({
    crv: "Ed25519",
    ext: true,
    key_ops: ["verify"],
    kty: "OKP",
    x: connect.hostPublicKeyX,
  });
  const fingerprint = encodeBase64Url(
    await crypto.subtle.digest("SHA-256", new TextEncoder().encode(canonicalJwk))
  );
  const signature = decodeCanonicalBase64Url(requiredString(record, "signature", "서버 신원 확인"));
  if (fingerprint !== connect.hostKeyFingerprint || !signature) {
    throw new Error("중앙 서버 공개 키 지문이 올바르지 않습니다.");
  }
  const key = await crypto.subtle.importKey(
    "jwk",
    { ...jwk, alg: "EdDSA" } as JsonWebKey,
    { name: "Ed25519" },
    false,
    ["verify"]
  );
  const transcript = [
    "AA-SERVER-CHALLENGE-1",
    connect.serverId,
    window.location.origin,
    challenge,
    String(issuedAt),
  ].join("\n");
  if (
    !(await crypto.subtle.verify(
      { name: "Ed25519" },
      key,
      signature,
      new TextEncoder().encode(transcript)
    ))
  ) {
    throw new Error("중앙 서버 서명을 확인하지 못했습니다.");
  }
}

export async function fetchCentralOwnerRooms(connect: CentralOwnerConnect, deviceToken: string, signal?: AbortSignal) {
  const payload = parseStrictRoomDirectory(await ownerRequest(connect, deviceToken, "directory", {}, signal));
  if (payload.server_id !== connect.serverId) throw new Error("선택한 서버와 방 목록이 일치하지 않습니다.");
  return payload;
}

export function openCentralOwnerDirectoryStream(
  connect: CentralOwnerConnect, deviceToken: string, signal: AbortSignal
) {
  if (!normalize(connect)) throw new ApiError(401, "서버 접속이 만료됐어요. 계정에서 서버를 다시 열어 주세요.", "central_connect_invalid");
  signal.throwIfAborted();
  return fetch("/api/central-owner/events", {
    method: "POST", cache: "no-store", credentials: "omit", redirect: "error", referrerPolicy: "no-referrer", signal,
    headers: { "content-type": "application/json", "x-device-token": deviceToken },
    body: JSON.stringify({ grant_token: connect.grantToken, generation: connect.generation }),
  });
}

export async function createCentralOwnerRoom(
  connect: CentralOwnerConnect, deviceToken: string,
  requestId: string, roomId: string, label: string
) {
  const payload = parseStrictRoomCreateResponse(await ownerRequest(connect, deviceToken, "rooms", {
    request_id: requestId, room_id: roomId, label,
  }));
  if (payload.server_id !== connect.serverId) throw new Error("선택한 서버와 생성된 방이 일치하지 않습니다.");
  return payload;
}

async function ownerRequest(
  connect: CentralOwnerConnect, deviceToken: string, route: string, body: Record<string, unknown> = {}, signal?: AbortSignal
): Promise<unknown> {
  if (!normalize(connect)) throw new ApiError(401, "서버 접속이 만료됐어요. 계정에서 서버를 다시 열어 주세요.", "central_connect_invalid");
  const response = await fetch(`/api/central-owner/${route}`, {
    method: "POST", cache: "no-store", credentials: "omit", redirect: "error", referrerPolicy: "no-referrer", signal,
    headers: { "content-type": "application/json", "x-device-token": deviceToken },
    body: JSON.stringify({ grant_token: connect.grantToken, generation: connect.generation, ...body }),
  });
  const payload = await response.json().catch(() => null);
  if (!response.ok) throw new ApiError(response.status,
    typeof payload?.error === "string" ? payload.error : payload?.error?.message || "서버 작업을 완료하지 못했어요.",
    payload?.code || payload?.error?.code || "");
  return payload;
}

const WORKSPACE_KEY = "agentsassemble.central-owner-workspace.v1";

export function loadCentralOwnerWorkspace(): CentralOwnerConnect | null {
  const raw = sessionStorage.getItem(WORKSPACE_KEY);
  if (!raw) return null;
  try {
    const record = JSON.parse(raw);
    const connect = record.origin === window.location.origin ? normalize(record.connect) : null;
    if (connect) return connect;
  } catch { /* Invalid custody is removed; it never becomes local authority. */ }
  sessionStorage.removeItem(WORKSPACE_KEY);
  return null;
}

export function persistCentralOwnerWorkspace(connect: CentralOwnerConnect | null) {
  if (!connect) { sessionStorage.removeItem(WORKSPACE_KEY); return; }
  if (!normalize(connect)) throw new Error("서버 접속이 만료됐어요.");
  sessionStorage.setItem(WORKSPACE_KEY, JSON.stringify({ origin: window.location.origin, connect }));
}

export async function enterCentralOwnerRoom(
  connect: CentralOwnerConnect,
  deviceToken: string,
  roomId: string,
  roomUid: string
) {
  const payload = parseOperatorPairingRedeemResponse(await ownerRequest(connect, deviceToken, "room", {
    room_id: roomId, room_uid: roomUid,
  }));
  if (payload.central_owner !== true || payload.server_id !== connect.serverId ||
      payload.meeting_id !== roomId || payload.room_uid !== roomUid) {
    throw new Error("선택한 서버·방과 발급된 접속권이 일치하지 않습니다.");
  }
  return payload;
}
