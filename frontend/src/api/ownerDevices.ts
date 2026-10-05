import { fetchOwnerSession } from "../lib/ownerSessionTransport";
import type { UserProfileIdentity } from "./userProfile";
import type { OwnerDevices } from "../types/generated/OwnerDevices";
import type { OwnerDeviceSession } from "../types/generated/OwnerDeviceSession";
import type { RevokeOwnerDevices } from "../types/generated/RevokeOwnerDevices";
import { validateHostName, validateHostOs } from "../lib/central/registrationProof";
import { assertExactKeys, strictRecord, stringField } from "../lib/strictJsonContract";
import { fetchJsonServerOperator, postJsonServerOperator, responseError, serverOwnerSessionHeaders } from "./http";

function parseDevices(value: unknown): OwnerDevices {
  const record = strictRecord(value, "기기 목록");
  assertExactKeys(record, ["sessions"], "기기 목록");
  if (!Array.isArray(record.sessions)) throw new Error("기기 목록이 올바르지 않아요.");
  const ids = new Set<string>();
  const sessions = record.sessions.map(value => {
    const row = strictRecord(value, "연결된 기기");
    assertExactKeys(row, ["session_id", "device_name", "browser", "os", "last_connected_at", "current", "connected", "revocable", "kind"], "연결된 기기");
    const id = stringField(row, "session_id", "연결된 기기");
    if (!(row.kind === "host" ? id === "host" : /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(id)) ||
        ids.has(id) || !["host", "owner", "pairing"].includes(String(row.kind)) ||
        typeof row.current !== "boolean" || typeof row.revocable !== "boolean" ||
        !(row.connected === null || typeof row.connected === "boolean") ||
        !(row.last_connected_at === null || Number.isSafeInteger(row.last_connected_at) && Number(row.last_connected_at) > 0 && Number(row.last_connected_at) <= 8640000000000) ||
        (row.kind === "host") === row.revocable) throw new Error("기기 목록의 상태를 확인하지 못했어요.");
    ids.add(id);
    for (const key of ["device_name", "browser", "os"]) {
      const text = stringField(row, key, "연결된 기기");
      if (row.kind === "host" && key === "device_name") {
        validateHostName(text);
        continue;
      }
      if (new TextEncoder().encode(text).length > 160 || /[\u0000-\u001f\u007f-\u009f]/.test(text)) throw new Error("기기 설명이 올바르지 않아요.");
    }
    if (row.kind === "host") validateHostOs(row.os);
    return row as OwnerDeviceSession;
  });
  return { sessions };
}

export async function listOwnerDevices(identity: UserProfileIdentity, signal?: AbortSignal): Promise<OwnerDevices> {
  if (!identity.centralSession) return parseDevices(await fetchJsonServerOperator("/api/owner-sessions", undefined, signal));
  const response = await fetchOwnerSession(identity.centralSession.sessionToken, "/api/central-owner/sessions", {
    cache: "no-store", credentials: "omit", redirect: "error", referrerPolicy: "no-referrer", signal,
    headers: serverOwnerSessionHeaders(identity.centralSession, identity.deviceToken || ""),
  });
  if (!response.ok) throw await responseError(response);
  return parseDevices(await response.json());
}

export async function revokeOwnerDevices(identity: UserProfileIdentity, body: RevokeOwnerDevices) {
  let payload: unknown;
  if (identity.centralSession) {
    const response = await fetchOwnerSession(identity.centralSession.sessionToken, "/api/central-owner/sessions/revoke", {
      method: "POST", cache: "no-store", credentials: "omit", redirect: "error", referrerPolicy: "no-referrer",
      headers: { ...serverOwnerSessionHeaders(identity.centralSession, identity.deviceToken || ""), "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!response.ok) throw await responseError(response);
    payload = await response.json();
  } else {
    payload = await postJsonServerOperator("/api/owner-sessions/revoke", body);
  }
  const record = strictRecord(payload, "기기 해제");
  assertExactKeys(record, ["status", "revoked_count"], "기기 해제");
  if (record.status !== "revoked" || !Number.isSafeInteger(record.revoked_count) || Number(record.revoked_count) < 0) {
    throw new Error("기기 해제 결과를 확인하지 못했어요.");
  }
}
