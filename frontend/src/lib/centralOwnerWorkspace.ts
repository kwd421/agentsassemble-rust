import { ApiError } from "./apiErrors";
import { browserDeviceDescription } from "./ownerDeviceDescription";
import { type CentralOwnerConnect } from "./centralOwnerConnect";
import { parseStrictRoomDirectory, parseStrictRoomCreateResponse } from "./roomDirectoryContract";
import { parseOperatorPairingRedeemResponse } from "./roomAdmissionContract";
import { assertExactKeys, requiredString, strictRecord } from "./strictJsonContract";

export type CentralOwnerWorkspace = Omit<CentralOwnerConnect, "grantToken" | "expiresAt"> & {
  sessionToken: string;
  sessionId: string;
};

// Previous releases stored root custody. A new page must obtain fresh central proof.
export function clearStoredCentralOwnerWorkspace() {
  sessionStorage.removeItem("agentsassemble.central-owner-workspace.v2");
}

export async function exchangeCentralOwnerSession(connect: CentralOwnerConnect, deviceToken: string): Promise<CentralOwnerWorkspace> {
  const record = strictRecord(await request("session", deviceToken, {
    grant_token: connect.grantToken, generation: connect.generation, device: browserDeviceDescription(),
  }), "서버 세션");
  assertExactKeys(record, ["session_token", "session_id", "server_id", "generation"], "서버 세션");
  const session: CentralOwnerWorkspace = {
    sessionToken: requiredString(record, "session_token", "서버 세션"),
    sessionId: requiredString(record, "session_id", "서버 세션"),
    serverId: requiredString(record, "server_id", "서버 세션"),
    generation: Number(record.generation),
    hostPublicKeyX: connect.hostPublicKeyX, hostKeyFingerprint: connect.hostKeyFingerprint,
  };
  if (!/^aaos1\.[A-Za-z0-9_-]{43}$/.test(session.sessionToken) ||
      !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(session.sessionId) ||
      session.serverId !== connect.serverId || session.generation !== connect.generation) {
    throw new Error("선택한 서버와 발급된 세션이 일치하지 않습니다.");
  }
  return session;
}

function sessionBody(session: CentralOwnerWorkspace) {
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
