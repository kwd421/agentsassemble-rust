/**
 * Join-to-registry handoff for joined member hosts.
 *
 * A fresh invite join — the person explicitly accepted a new invite for one
 * host — mints a live admission that already carries the host's current
 * registration epoch and host key. It is the only event that may replace the
 * host identity the member host registry refuses after an epoch or host-key
 * change, so the consent is recorded here instead of published as an event:
 * the registry is often not mounted yet (the join page replaces the app) and
 * an event published before it mounts would be missed.
 *
 * The record is narrow: one per person and server, naming the exact
 * consented incarnation. It is single-use, expires within minutes, and is
 * cleared with the other per-person member state on sign-out and account
 * switch. The live admission stays in memory only — bearers and channels
 * never reach the durable store.
 */

import { saveConsentedMemberAnchor, type MemberAnchorRoom, type MemberHostIdentity } from "./memberAnchors";

export const MEMBER_JOIN_HANDOFF_TTL_MS = 10 * 60 * 1000;

const KEY_PREFIX = "agentsassemble.memberJoinHandoffs.v1:";
const MAX_HANDOFFS = 20;

export type MemberJoinHandoff = {
  server_id: string;
  registration_epoch: string;
  host_key_fingerprint: string;
  expires_at: number;
};

export type MemberJoinHandoffConsumption = {
  handoff: MemberJoinHandoff;
  /** The live admission the consented join produced. */
  admission: unknown;
};

// The live admission each consented join produced, keyed by the consented
// incarnation. Memory only: the durable store never holds a bearer.
const admissions = new Map<string, unknown>();

function keyFor(personId: string): string {
  return `${KEY_PREFIX}${personId}`;
}

function admissionKey(personId: string, serverId: string, registrationEpoch: string): string {
  return `${personId}\0${serverId}\0${registrationEpoch}`;
}

function cleanId(value: string): string {
  return value.trim().slice(0, 200);
}

function parseHandoff(value: unknown): MemberJoinHandoff | null {
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const server_id = typeof record.server_id === "string" ? cleanId(record.server_id) : "";
  const registration_epoch = typeof record.registration_epoch === "string" ? cleanId(record.registration_epoch) : "";
  const host_key_fingerprint = typeof record.host_key_fingerprint === "string"
    ? record.host_key_fingerprint.trim().slice(0, 400) : "";
  const expires_at = record.expires_at;
  if (!server_id || !registration_epoch || !host_key_fingerprint) return null;
  if (typeof expires_at !== "number" || !Number.isSafeInteger(expires_at)) return null;
  return { server_id, registration_epoch, host_key_fingerprint, expires_at };
}

function readHandoffs(personId: string): MemberJoinHandoff[] {
  if (!personId) return [];
  try {
    const raw = window.localStorage.getItem(keyFor(personId));
    if (!raw) return [];
    const parsed = JSON.parse(raw) as { handoffs?: unknown };
    if (!parsed || !Array.isArray(parsed.handoffs)) return [];
    const handoffs: MemberJoinHandoff[] = [];
    for (const entry of parsed.handoffs) {
      const handoff = parseHandoff(entry);
      if (handoff) handoffs.push(handoff);
    }
    return handoffs;
  } catch {
    return [];
  }
}

function writeHandoffs(personId: string, handoffs: MemberJoinHandoff[]): void {
  try {
    if (handoffs.length === 0) {
      window.localStorage.removeItem(keyFor(personId));
      return;
    }
    window.localStorage.setItem(keyFor(personId), JSON.stringify({ handoffs }));
  } catch {
    // The handoff is best-effort: without it the registry keeps the refusal
    // and the person re-joins through a new invite. The join itself never
    // fails over a display cache.
    if (import.meta.env.DEV) console.debug("[member-join] handoff not stored", { personId });
  }
}

/**
 * Note an explicit invite join's consent for one host incarnation: the
 * anchor follows the consented identity (the previous epoch's anchor is
 * stale) and the member host registry may take the live admission over
 * through the handoff. Only the invite join path calls this — an ordinary
 * room select on a cached bearer never consents to anything.
 */
export function noteMemberJoinConsent(
  personId: string,
  identity: MemberHostIdentity,
  admission: unknown,
  joinedRooms: MemberAnchorRoom[] = [],
): void {
  try {
    saveConsentedMemberAnchor(personId, identity, joinedRooms);
  } catch {
    // The anchor is a display cache: the join itself succeeded. Without it
    // the registry keeps refusing a host whose identity it cannot confirm.
    if (import.meta.env.DEV) console.debug("[member-join] consented anchor not stored", { personId });
  }
  recordMemberJoinHandoff(personId, identity, admission);
}

/**
 * Record an explicit invite join's consent for one host incarnation. Only
 * the invite join path calls this: an ordinary room select on a cached
 * bearer never creates a handoff.
 */
export function recordMemberJoinHandoff(
  personId: string,
  identity: { server_id: string; registration_epoch: string; host_key_fingerprint: string },
  admission: unknown,
): void {
  if (!personId || !identity?.server_id || !identity.registration_epoch || !identity.host_key_fingerprint) return;
  const handoff: MemberJoinHandoff = {
    server_id: cleanId(identity.server_id),
    registration_epoch: cleanId(identity.registration_epoch),
    host_key_fingerprint: identity.host_key_fingerprint.trim().slice(0, 400),
    expires_at: Date.now() + MEMBER_JOIN_HANDOFF_TTL_MS,
  };
  const now = Date.now();
  const kept = readHandoffs(personId).filter((entry) =>
    entry.server_id !== handoff.server_id && entry.expires_at > now);
  kept.push(handoff);
  writeHandoffs(personId, kept.slice(-MAX_HANDOFFS));
  // One live admission per host: a newer consent replaces the previous
  // incarnation's carrier instead of keeping it alive.
  const prefix = `${personId}\0${handoff.server_id}\0`;
  for (const key of [...admissions.keys()]) {
    if (key.startsWith(prefix)) admissions.delete(key);
  }
  admissions.set(admissionKey(personId, handoff.server_id, handoff.registration_epoch), admission);
}

/**
 * Consume the handoff of one host, but only when it names exactly the
 * incarnation the caller currently sees: same person, same server, same
 * registration epoch and host key. A mismatched, expired or already-spent
 * handoff does nothing.
 */
export function takeMemberJoinHandoff(
  personId: string,
  serverId: string,
  expected: { registration_epoch: string; host_key_fingerprint: string },
): MemberJoinHandoffConsumption | null {
  if (!personId || !serverId) return null;
  const now = Date.now();
  const stored = readHandoffs(personId);
  const live = stored.filter((entry) => entry.expires_at > now);
  if (live.length !== stored.length) {
    writeHandoffs(personId, live);
    for (const entry of stored) {
      if (entry.expires_at > now) continue;
      admissions.delete(admissionKey(personId, entry.server_id, entry.registration_epoch));
    }
  }
  const handoff = live.find((entry) => entry.server_id === serverId);
  if (!handoff) return null;
  if (handoff.registration_epoch !== expected.registration_epoch) return null;
  // The host key must match exactly: an unknown key never consumes a consent.
  if (!expected.host_key_fingerprint || handoff.host_key_fingerprint !== expected.host_key_fingerprint) return null;
  const admission = admissions.get(admissionKey(personId, serverId, handoff.registration_epoch));
  if (admission === undefined) return null;
  // Single use: the consent is spent, and the record never serves a second
  // registry.
  writeHandoffs(personId, live.filter((entry) => entry !== handoff));
  admissions.delete(admissionKey(personId, serverId, handoff.registration_epoch));
  return { handoff, admission };
}

/** Drops every handoff of one person (sign-out, account switch). */
export function clearMemberJoinHandoffs(personId: string): void {
  if (!personId) return;
  const prefix = `${personId}\0`;
  for (const key of [...admissions.keys()]) {
    if (key.startsWith(prefix)) admissions.delete(key);
  }
  writeHandoffs(personId, []);
}
