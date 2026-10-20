import { strictRecord, assertExactKeys, requiredString } from "../strictJsonContract";

// Durable per-host re-entry anchor for joined member hosts. Display only:
// server identity pins plus the last known room list and hide state. Session
// bearers, grants, browser credentials and private keys must never enter this
// store; exact-keys validation below rejects any such field outright.
export type MemberAnchorRoom = { room_id: string; name: string };
/** The host identity an anchor pins: server, registration epoch, host key. */
export type MemberHostIdentity = {
  server_id: string;
  registration_epoch: string;
  host_key_fingerprint: string;
  endpoint_origin: string;
  endpoint_generation: number;
};
export type MemberHostAnchor = MemberHostIdentity & {
  rooms: MemberAnchorRoom[];
  hidden: boolean;
  updated_at: number;
};

const KEY_PREFIX = "agentsassemble.memberAnchors.v1:";
const ROOM_LIMIT = 50;

function keyFor(personId: string): string {
  return `${KEY_PREFIX}${personId}`;
}

function cleanId(value: string): string {
  return value.trim().slice(0, 200);
}

function parseRoom(value: unknown): MemberAnchorRoom | null {
  try {
    const record = strictRecord(value, "참가 중인 방");
    assertExactKeys(record, ["room_id", "name"], "참가 중인 방");
    const room_id = cleanId(requiredString(record, "room_id", "참가 중인 방"));
    const name = String(record.name || "").normalize("NFC").replace(/[\p{Cc}\p{Cf}]/gu, " ").replace(/\s+/gu, " ").trim().slice(0, 80);
    if (!room_id || room_id.length > 200) return null;
    return { room_id, name };
  } catch {
    return null;
  }
}

function parseAnchor(value: unknown): MemberHostAnchor | null {
  try {
    const record = strictRecord(value, "참가 서버 기록");
    assertExactKeys(record, ["server_id", "registration_epoch", "host_key_fingerprint",
      "endpoint_origin", "endpoint_generation", "rooms", "hidden", "updated_at"], "참가 서버 기록");
    const server_id = cleanId(requiredString(record, "server_id", "참가 서버 기록"));
    const registration_epoch = cleanId(requiredString(record, "registration_epoch", "참가 서버 기록"));
    const host_key_fingerprint = cleanId(requiredString(record, "host_key_fingerprint", "참가 서버 기록"));
    const endpoint_origin = requiredString(record, "endpoint_origin", "참가 서버 기록");
    if (!server_id || !registration_epoch || !host_key_fingerprint) return null;
    if (server_id.length > 200 || registration_epoch.length > 200 || host_key_fingerprint.length > 400) return null;
    const endpoint = new URL(endpoint_origin);
    if (endpoint.protocol !== "https:" || endpoint_origin.length > 400 || endpoint_origin !== endpoint.origin) return null;
    if (typeof record.endpoint_generation !== "number" || !Number.isSafeInteger(record.endpoint_generation) ||
        record.endpoint_generation < 1) return null;
    if (!Array.isArray(record.rooms) || record.rooms.length > ROOM_LIMIT) return null;
    const rooms: MemberAnchorRoom[] = [];
    const seen = new Set<string>();
    for (const entry of record.rooms) {
      const room = parseRoom(entry);
      if (!room || seen.has(room.room_id)) return null;
      seen.add(room.room_id);
      rooms.push(room);
    }
    if (typeof record.hidden !== "boolean") return null;
    if (typeof record.updated_at !== "number" || !Number.isSafeInteger(record.updated_at) || record.updated_at < 0) return null;
    return {
      server_id, registration_epoch, host_key_fingerprint, endpoint_origin,
      endpoint_generation: record.endpoint_generation, rooms, hidden: record.hidden, updated_at: record.updated_at,
    };
  } catch {
    return null;
  }
}

function readRaw(personId: string): unknown[] {
  if (!personId) return [];
  try {
    const raw = window.localStorage.getItem(keyFor(personId));
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || !Array.isArray((parsed as { anchors?: unknown }).anchors)) return [];
    return (parsed as { anchors: unknown[] }).anchors;
  } catch {
    return [];
  }
}

export function loadMemberAnchors(personId: string): MemberHostAnchor[] {
  const anchors: MemberHostAnchor[] = [];
  const seen = new Set<string>();
  for (const entry of readRaw(personId)) {
    const anchor = parseAnchor(entry);
    if (!anchor || seen.has(anchor.server_id)) continue;
    seen.add(anchor.server_id);
    anchors.push(anchor);
  }
  return anchors;
}

export function loadMemberAnchor(personId: string, serverId: string): MemberHostAnchor | null {
  return loadMemberAnchors(personId).find((anchor) => anchor.server_id === serverId) ?? null;
}

export const MEMBER_ANCHORS_CHANGED_EVENT = "agentsassemble:member-anchors-changed";

/**
 * The stored anchor no longer matches the server identity the person is
 * on: another registration epoch or another host key. The host is parked
 * with this reason instead of being retried, and the stored anchor stays
 * as the refusal baseline until fresh consent overwrites it.
 */
export class MemberAnchorMismatchError extends Error {
  constructor() {
    super("참가 서버 정보가 바뀌었어요. 새 초대로 다시 참가해 주세요.");
    this.name = "MemberAnchorMismatchError";
  }
}

function announceAnchorsChanged(): void {
  try { window.dispatchEvent(new Event(MEMBER_ANCHORS_CHANGED_EVENT)); } catch { /* display cache stays local */ }
}

function writeAnchors(personId: string, anchors: MemberHostAnchor[]): void {
  try {
    const serialized = JSON.stringify({ anchors });
    window.localStorage.setItem(keyFor(personId), serialized);
    if (window.localStorage.getItem(keyFor(personId)) !== serialized) {
      throw new Error("참가 서버 기록을 저장하지 못했어요.");
    }
    announceAnchorsChanged();
  } catch {
    throw new Error("참가 서버 기록을 저장하지 못했어요. 브라우저 저장소 접근을 허용한 뒤 다시 시도해 주세요.");
  }
}

export function saveMemberAnchor(personId: string, anchor: MemberHostAnchor): void {
  const parsed = parseAnchor(anchor);
  if (!parsed || !personId) throw new Error("참가 서버 기록을 확인하지 못했어요.");
  const next = loadMemberAnchors(personId).filter((entry) => entry.server_id !== parsed.server_id);
  next.push(parsed);
  writeAnchors(personId, next);
}

export function removeMemberAnchor(personId: string, serverId: string): void {
  if (!personId) return;
  const next = loadMemberAnchors(personId).filter((anchor) => anchor.server_id !== serverId);
  if (next.length === loadMemberAnchors(personId).length) return;
  if (next.length === 0) {
    try { window.localStorage.removeItem(keyFor(personId)); } catch { /* authoritative removal stays effective */ }
    announceAnchorsChanged();
    return;
  }
  writeAnchors(personId, next);
}

/**
 * Fresh consent names the host incarnation the person just accepted: the
 * anchor follows it (the previous epoch's identity is stale) while the known
 * rooms and the hide state stay. Display cache only — a rejected write is
 * reported to the caller, never to the person who already joined.
 */
export function saveConsentedMemberAnchor(
  personId: string,
  identity: MemberHostIdentity,
  joinedRooms: MemberAnchorRoom[] = [],
): void {
  if (!personId) return;
  const previous = loadMemberAnchor(personId, identity.server_id);
  const rooms: MemberAnchorRoom[] = [];
  for (const room of [...joinedRooms, ...(previous?.rooms ?? [])]) {
    if (rooms.length >= ROOM_LIMIT) break;
    if (rooms.some((entry) => entry.room_id === room.room_id)) continue;
    rooms.push(room);
  }
  saveMemberAnchor(personId, {
    ...identity,
    rooms,
    hidden: previous?.hidden ?? false,
    updated_at: Date.now(),
  });
}

export function removeMemberAnchorRoom(personId: string, serverId: string, roomId: string): void {
  const anchor = loadMemberAnchor(personId, serverId);
  if (!anchor || !anchor.rooms.some((room) => room.room_id === roomId)) return;
  saveMemberAnchor(personId, { ...anchor, rooms: anchor.rooms.filter((room) => room.room_id !== roomId), updated_at: Date.now() });
}

export function clearMemberAnchors(personId: string): void {
  if (!personId) return;
  try { window.localStorage.removeItem(keyFor(personId)); } catch { /* session clearing stays authoritative */ }
  announceAnchorsChanged();
}
