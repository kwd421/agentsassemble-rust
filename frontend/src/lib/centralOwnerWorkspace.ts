import { ApiError } from "./apiErrors";
import { type CentralOwnerConnect } from "./centralOwnerConnect";
import { parseStrictRoomDirectory, parseStrictRoomCreateResponse } from "./roomDirectoryContract";
import { parseOperatorPairingRedeemResponse } from "./roomAdmissionContract";
import { assertExactKeys, requiredString, strictRecord } from "./strictJsonContract";

export type CentralOwnerWorkspace = Omit<CentralOwnerConnect, "grantToken" | "expiresAt"> & {
  sessionToken: string;
  expiresAt: number;
  leaseExpiresAt: number;
};

const WORKSPACE_KEY = "agentsassemble.central-owner-workspace.v2";

function normalize(value: unknown): CentralOwnerWorkspace | null {
  try {
    const record = strictRecord(value, "서버 세션");
    assertExactKeys(record, ["sessionToken", "serverId", "generation", "expiresAt", "leaseExpiresAt", "hostPublicKeyX", "hostKeyFingerprint"], "서버 세션");
    const result: CentralOwnerWorkspace = {
      sessionToken: requiredString(record, "sessionToken", "서버 세션"),
      serverId: requiredString(record, "serverId", "서버 세션"),
      generation: Number(record.generation), expiresAt: Number(record.expiresAt),
      leaseExpiresAt: Number(record.leaseExpiresAt),
      hostPublicKeyX: requiredString(record, "hostPublicKeyX", "서버 세션"),
      hostKeyFingerprint: requiredString(record, "hostKeyFingerprint", "서버 세션"),
    };
    if (!/^aaos1\.[A-Za-z0-9_-]{43}$/.test(result.sessionToken) ||
        !/^[A-Za-z0-9._:-]{1,200}$/.test(result.serverId) ||
        !Number.isSafeInteger(result.generation) || result.generation < 1 ||
        !Number.isSafeInteger(result.expiresAt) || result.expiresAt <= Date.now() / 1000 ||
        !Number.isSafeInteger(result.leaseExpiresAt) || result.leaseExpiresAt > result.expiresAt ||
        !/^[A-Za-z0-9_-]{43}$/.test(result.hostPublicKeyX) ||
        !/^[A-Za-z0-9_-]{43}$/.test(result.hostKeyFingerprint)) return null;
    return result;
  } catch { return null; }
}

export function loadCentralOwnerWorkspace(): CentralOwnerWorkspace | null {
  const raw = sessionStorage.getItem(WORKSPACE_KEY);
  if (!raw) return null;
  try {
    const record = JSON.parse(raw);
    const session = record.origin === window.location.origin ? normalize(record.session) : null;
    if (session) return session;
  } catch { /* Invalid custody never becomes local authority. */ }
  sessionStorage.removeItem(WORKSPACE_KEY);
  return null;
}

export function persistCentralOwnerWorkspace(session: CentralOwnerWorkspace | null) {
  if (!session) { sessionStorage.removeItem(WORKSPACE_KEY); return; }
  if (!normalize(session)) throw new Error("서버 접속이 만료됐어요.");
  sessionStorage.setItem(WORKSPACE_KEY, JSON.stringify({ origin: window.location.origin, session }));
}

export async function exchangeCentralOwnerSession(connect: CentralOwnerConnect, deviceToken: string): Promise<CentralOwnerWorkspace> {
  const record = strictRecord(await request("session", deviceToken, { grant_token: connect.grantToken, generation: connect.generation }), "서버 세션");
  assertExactKeys(record, ["session_token", "server_id", "generation", "expires_at", "session_expires_at"], "서버 세션");
  const session = normalize({ sessionToken: record.session_token, serverId: record.server_id,
    generation: record.generation, leaseExpiresAt: record.expires_at, expiresAt: record.session_expires_at,
    hostPublicKeyX: connect.hostPublicKeyX, hostKeyFingerprint: connect.hostKeyFingerprint });
  if (!session || session.serverId !== connect.serverId || session.generation !== connect.generation ||
      session.leaseExpiresAt <= Date.now() / 1000 || session.leaseExpiresAt > Date.now() / 1000 + 60) {
    throw new Error("선택한 서버와 발급된 세션이 일치하지 않습니다.");
  }
  return session;
}

function sessionBody(session: CentralOwnerWorkspace) {
  if (!normalize(session)) throw new ApiError(401, "서버 접속이 만료됐어요. 계정에서 서버를 다시 열어 주세요.", "central_session_invalid");
  return { session_token: session.sessionToken, generation: session.generation };
}

export function openCentralOwnerDirectoryStream(session: CentralOwnerWorkspace, deviceToken: string, signal: AbortSignal) {
  signal.throwIfAborted();
  return fetch("/api/central-owner/events", { method: "POST", cache: "no-store", credentials: "omit",
    redirect: "error", referrerPolicy: "no-referrer", signal,
    headers: { "content-type": "application/json", "x-device-token": deviceToken },
    body: JSON.stringify(sessionBody(session)) });
}

async function request(route: string, deviceToken: string, body: Record<string, unknown>, signal?: AbortSignal): Promise<unknown> {
  const response = await fetch(`/api/central-owner/${route}`, { method: "POST", cache: "no-store",
    credentials: "omit", redirect: "error", referrerPolicy: "no-referrer", signal,
    headers: { "content-type": "application/json", "x-device-token": deviceToken }, body: JSON.stringify(body) });
  const payload = await response.json().catch(() => null);
  if (!response.ok) throw new ApiError(response.status,
    typeof payload?.error === "string" ? payload.error : payload?.error?.message || "서버 작업을 완료하지 못했어요.",
    payload?.code || payload?.error?.code || "");
  return payload;
}

export async function fetchCentralOwnerRooms(session: CentralOwnerWorkspace, deviceToken: string, signal?: AbortSignal) {
  const payload = parseStrictRoomDirectory(await request("directory", deviceToken, sessionBody(session), signal));
  if (payload.server_id !== session.serverId) throw new Error("선택한 서버와 방 목록이 일치하지 않습니다.");
  return payload;
}

export async function createCentralOwnerRoom(session: CentralOwnerWorkspace, deviceToken: string, requestId: string, roomId: string, label: string) {
  const payload = parseStrictRoomCreateResponse(await request("rooms", deviceToken,
    { ...sessionBody(session), request_id: requestId, room_id: roomId, label }));
  if (payload.server_id !== session.serverId) throw new Error("선택한 서버와 생성된 방이 일치하지 않습니다.");
  return payload;
}

export async function enterCentralOwnerRoom(session: CentralOwnerWorkspace, deviceToken: string, roomId: string, roomUid: string) {
  const payload = parseOperatorPairingRedeemResponse(await request("room", deviceToken, { ...sessionBody(session), room_id: roomId, room_uid: roomUid }));
  if (payload.central_owner !== true || payload.server_id !== session.serverId || payload.meeting_id !== roomId || payload.room_uid !== roomUid) {
    throw new Error("선택한 서버·방과 발급된 접속권이 일치하지 않습니다.");
  }
  return payload;
}
