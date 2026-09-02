import { ApiError } from "./apiErrors";
import type { RoomGuestSession } from "./roomGuestSession";
import { createSecureRequestId } from "./secureRequestId";

export const ROOM_ADMISSION_INTENT_STORAGE_KEY =
  "agentsassemble.roomAdmissionIntent.v1";

const CANONICAL_UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const SHA256_HEX_PATTERN = /^[0-9a-f]{64}$/;
const NIL_UUID = "00000000-0000-0000-0000-000000000000";
const MAX_STORED_INTENT_BYTES = 8 * 1024;
const INTENT_UNAVAILABLE_MESSAGE =
  "이 브라우저에서는 입장 재시도 정보를 안전하게 보관할 수 없습니다.";
const DEFINITIVE_INTENT_END_CODES = new Set([
  "admission_session_unavailable",
  "bad_request",
  "browser_credential_invalid",
  "idempotency_conflict",
  "invite_token_required",
  "meeting_mismatch",
  "participant_identity_conflict",
  "participant_type_invalid",
  "payload_too_large",
  "request_id_invalid",
  "request_id_required",
]);

export type RoomAdmissionIntent = {
  requestId: string;
  meetingId: string;
  displayName: string;
  avatarImage: string;
  clientId: string;
  participantType: "human";
};

type StoredPendingRoomAdmissionIntent = RoomAdmissionIntent & {
  version: 1;
  inviteCredentialFingerprint: string;
  browserCredentialFingerprint: string;
};

type StoredSettledRoomAdmissionIntent = {
  version: 1;
  state: "settled";
  inviteCredentialFingerprint: string;
  browserCredentialFingerprint: string;
  clientId: string;
  outcome: "completed_session" | "terminal";
  terminalCode: string;
};

type StoredRoomAdmissionIntent =
  | StoredPendingRoomAdmissionIntent
  | StoredSettledRoomAdmissionIntent;

type RoomAdmissionIntentContext = {
  inviteToken: string;
  browserCredential: string;
  clientId: string;
};

type NewRoomAdmissionIntent = RoomAdmissionIntentContext &
  Omit<RoomAdmissionIntent, "requestId" | "clientId" | "participantType">;
type CompletedAdmissionSession = Pick<
  RoomGuestSession,
  "inviteToken" | "clientId" | "expiresAt"
>;
export type RoomAdmissionIntentResolution =
  | { kind: "pending"; intent: RoomAdmissionIntent }
  | { kind: "terminal"; code: string }
  | null;
export type RoomAdmissionSettlement =
  | { outcome: "completed_session" }
  | { outcome: "terminal"; code: string };

function unavailable(): never {
  throw new Error(INTENT_UNAVAILABLE_MESSAGE);
}

function canonicalUuid(value: string): boolean {
  return value !== NIL_UUID && CANONICAL_UUID_PATTERN.test(value);
}

function validStoredBinding(source: Record<string, unknown>): boolean {
  return (
    source.version === 1 &&
    typeof source.inviteCredentialFingerprint === "string" &&
    SHA256_HEX_PATTERN.test(source.inviteCredentialFingerprint) &&
    typeof source.browserCredentialFingerprint === "string" &&
    SHA256_HEX_PATTERN.test(source.browserCredentialFingerprint) &&
    typeof source.clientId === "string" &&
    source.clientId.trim() === source.clientId &&
    source.clientId.length > 0 &&
    source.clientId.length <= 128
  );
}

function validStoredIntent(value: unknown): value is StoredRoomAdmissionIntent {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const source = value as Record<string, unknown>;
  const keys = Object.keys(source).sort().join("\n");
  const settledKeys = [
    "browserCredentialFingerprint",
    "clientId",
    "inviteCredentialFingerprint",
    "outcome",
    "state",
    "terminalCode",
    "version",
  ]
    .sort()
    .join("\n");
  if (keys === settledKeys) {
    return (
      validStoredBinding(source) &&
      source.state === "settled" &&
      (source.outcome === "completed_session" || source.outcome === "terminal") &&
      typeof source.terminalCode === "string" &&
      (source.outcome === "completed_session"
        ? source.terminalCode === ""
        : DEFINITIVE_INTENT_END_CODES.has(source.terminalCode))
    );
  }
  if (
    keys !==
    [
      "avatarImage",
      "browserCredentialFingerprint",
      "clientId",
      "displayName",
      "inviteCredentialFingerprint",
      "meetingId",
      "participantType",
      "requestId",
      "version",
    ]
      .sort()
      .join("\n")
  ) {
    return false;
  }
  return (
    validStoredBinding(source) &&
    source.participantType === "human" &&
    typeof source.requestId === "string" &&
    canonicalUuid(source.requestId) &&
    typeof source.meetingId === "string" &&
    source.meetingId.length > 0 &&
    source.meetingId.length <= 128 &&
    typeof source.displayName === "string" &&
    source.displayName.trim().length > 0 &&
    source.displayName.length <= 128 &&
    typeof source.avatarImage === "string" &&
    source.avatarImage.length <= 2048
  );
}

async function sha256Hex(value: string): Promise<string> {
  if (
    typeof globalThis.crypto?.subtle?.digest !== "function" ||
    typeof globalThis.TextEncoder !== "function"
  ) {
    unavailable();
  }
  try {
    const digest = await globalThis.crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(value)
    );
    return Array.from(new Uint8Array(digest), (byte) =>
      byte.toString(16).padStart(2, "0")
    ).join("");
  } catch {
    unavailable();
  }
}

function sessionStorageOwner(): Storage {
  try {
    return window.sessionStorage;
  } catch {
    unavailable();
  }
}

function readStoredIntent(storage: Storage): StoredRoomAdmissionIntent | null {
  let raw: string | null;
  try {
    raw = storage.getItem(ROOM_ADMISSION_INTENT_STORAGE_KEY);
  } catch {
    unavailable();
  }
  if (raw === null) return null;
  if (raw.length > MAX_STORED_INTENT_BYTES) unavailable();
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!validStoredIntent(parsed)) unavailable();
    return parsed;
  } catch {
    unavailable();
  }
}

function storedIntentMatches(
  stored: StoredRoomAdmissionIntent,
  context: RoomAdmissionIntentContext,
  fingerprints: Awaited<ReturnType<typeof expectedFingerprints>>
): boolean {
  return (
    stored.inviteCredentialFingerprint === fingerprints.inviteCredentialFingerprint &&
    stored.browserCredentialFingerprint === fingerprints.browserCredentialFingerprint &&
    stored.clientId === context.clientId
  );
}

function storedIntentUnchanged(
  observed: StoredRoomAdmissionIntent,
  current: StoredRoomAdmissionIntent
): boolean {
  return JSON.stringify(observed) === JSON.stringify(current);
}

function removeStoredIntent(storage: Storage): boolean {
  storage.removeItem(ROOM_ADMISSION_INTENT_STORAGE_KEY);
  return storage.getItem(ROOM_ADMISSION_INTENT_STORAGE_KEY) === null;
}

function tryRemoveStoredIntent(storage: Storage): boolean {
  try {
    return removeStoredIntent(storage);
  } catch {
    return false;
  }
}

function writeStoredIntent(
  storage: Storage,
  stored: StoredRoomAdmissionIntent
): boolean {
  const serialized = JSON.stringify(stored);
  if (serialized.length > MAX_STORED_INTENT_BYTES) unavailable();
  storage.setItem(ROOM_ADMISSION_INTENT_STORAGE_KEY, serialized);
  return storage.getItem(ROOM_ADMISSION_INTENT_STORAGE_KEY) === serialized;
}

function settledIntent(
  stored: StoredRoomAdmissionIntent,
  settlement: RoomAdmissionSettlement
): StoredSettledRoomAdmissionIntent {
  return {
    version: 1,
    state: "settled",
    inviteCredentialFingerprint: stored.inviteCredentialFingerprint,
    browserCredentialFingerprint: stored.browserCredentialFingerprint,
    clientId: stored.clientId,
    outcome: settlement.outcome,
    terminalCode: settlement.outcome === "terminal" ? settlement.code : "",
  };
}

function settlementMatches(
  stored: StoredSettledRoomAdmissionIntent,
  settlement: RoomAdmissionSettlement
): boolean {
  return (
    stored.outcome === settlement.outcome &&
    stored.terminalCode === (settlement.outcome === "terminal" ? settlement.code : "")
  );
}

function persistSettlementThenRemove(
  storage: Storage,
  stored: StoredRoomAdmissionIntent,
  settlement: RoomAdmissionSettlement
): boolean {
  if ("state" in stored) {
    if (!settlementMatches(stored, settlement)) return false;
  } else {
    const settled = settledIntent(stored, settlement);
    if (!validStoredIntent(settled)) return false;
    try {
      if (!writeStoredIntent(storage, settled)) {
        return tryRemoveStoredIntent(storage);
      }
    } catch {
      return tryRemoveStoredIntent(storage);
    }
  }
  // Failed removal leaves the durable settled record cleanup-only for a later load.
  return tryRemoveStoredIntent(storage);
}

function completedSessionContext(
  context: RoomAdmissionIntentContext,
  session: CompletedAdmissionSession | null | undefined
): RoomAdmissionIntentContext | undefined {
  if (
    !session ||
    !session.inviteToken ||
    Number.isNaN(Date.parse(session.expiresAt)) ||
    session.clientId !== context.clientId
  ) {
    return undefined;
  }
  return {
    inviteToken: session.inviteToken,
    browserCredential: context.browserCredential,
    clientId: session.clientId,
  };
}

async function expectedFingerprints(context: RoomAdmissionIntentContext) {
  if (!context.inviteToken || !context.browserCredential) unavailable();
  const [inviteCredentialFingerprint, browserCredentialFingerprint] =
    await Promise.all([
      sha256Hex(context.inviteToken),
      sha256Hex(context.browserCredential),
    ]);
  return { inviteCredentialFingerprint, browserCredentialFingerprint };
}

export async function loadRoomAdmissionIntent(
  context: RoomAdmissionIntentContext,
  completedSession?: CompletedAdmissionSession | null
): Promise<RoomAdmissionIntentResolution> {
  const storage = sessionStorageOwner();
  const stored = readStoredIntent(storage);
  if (!stored) return null;
  const fingerprints = await expectedFingerprints(context);
  const current = readStoredIntent(storage);
  if (!current) return null;
  if (!storedIntentUnchanged(stored, current)) unavailable();
  const matchesCurrent = storedIntentMatches(current, context, fingerprints);
  if ("state" in current) {
    if (!tryRemoveStoredIntent(storage)) unavailable();
    return matchesCurrent && current.outcome === "terminal"
      ? { kind: "terminal", code: current.terminalCode }
      : null;
  }
  if (!matchesCurrent) {
    const completed = completedSessionContext(context, completedSession);
    if (completed) {
      const completedFingerprints = await expectedFingerprints(completed);
      const afterCompletedFingerprint = readStoredIntent(storage);
      if (
        !afterCompletedFingerprint ||
        !storedIntentUnchanged(current, afterCompletedFingerprint)
      ) unavailable();
      if (storedIntentMatches(current, completed, completedFingerprints)) {
        if (
          !persistSettlementThenRemove(storage, current, {
            outcome: "completed_session",
          })
        ) unavailable();
        return null;
      }
    }
    unavailable();
  }
  const {
    version: _version,
    inviteCredentialFingerprint: _inviteFingerprint,
    browserCredentialFingerprint: _browserFingerprint,
    ...intent
  } = current;
  return { kind: "pending", intent };
}

export async function loadOrCreateRoomAdmissionIntent(
  input: NewRoomAdmissionIntent
): Promise<RoomAdmissionIntent> {
  const existing = await loadRoomAdmissionIntent(input);
  if (existing?.kind === "pending") return existing.intent;
  if (existing) unavailable();
  const fingerprints = await expectedFingerprints(input);
  let requestId: string;
  try {
    requestId = createSecureRequestId();
  } catch {
    unavailable();
  }
  const stored: StoredPendingRoomAdmissionIntent = {
    version: 1,
    ...fingerprints,
    requestId,
    meetingId: input.meetingId,
    displayName: input.displayName,
    avatarImage: input.avatarImage,
    clientId: input.clientId,
    participantType: "human",
  };
  if (!validStoredIntent(stored)) unavailable();
  const storage = sessionStorageOwner();
  if (readStoredIntent(storage)) unavailable();
  try {
    if (!writeStoredIntent(storage, stored)) unavailable();
  } catch {
    try {
      storage.removeItem(ROOM_ADMISSION_INTENT_STORAGE_KEY);
    } catch {
      // No network effect occurred, and the next explicit retry must revalidate storage.
    }
    unavailable();
  }
  return {
    requestId: stored.requestId,
    meetingId: stored.meetingId,
    displayName: stored.displayName,
    avatarImage: stored.avatarImage,
    clientId: stored.clientId,
    participantType: stored.participantType,
  };
}

export async function settleRoomAdmissionIntent(
  context: RoomAdmissionIntentContext,
  settlement: RoomAdmissionSettlement
): Promise<boolean> {
  let storage: Storage;
  let stored: StoredRoomAdmissionIntent | null;
  try {
    storage = sessionStorageOwner();
    stored = readStoredIntent(storage);
    if (!stored) return true;
    const fingerprints = await expectedFingerprints(context);
    const current = readStoredIntent(storage);
    if (!current) return true;
    if (
      !storedIntentUnchanged(stored, current) ||
      !storedIntentMatches(current, context, fingerprints)
    ) return false;
    stored = current;
  } catch {
    return false;
  }
  return persistSettlementThenRemove(storage, stored, settlement);
}

export function roomAdmissionFailureEndsIntentCustody(error: unknown): boolean {
  return error instanceof ApiError && DEFINITIVE_INTENT_END_CODES.has(error.code);
}
