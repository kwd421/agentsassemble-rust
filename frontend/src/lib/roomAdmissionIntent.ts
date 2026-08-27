import { ApiError } from "./apiErrors";
import {
  roomGuestSessionExpired,
  type RoomGuestSession,
} from "./roomGuestSession";

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

type StoredRoomAdmissionIntent = RoomAdmissionIntent & {
  version: 1;
  inviteCredentialFingerprint: string;
  browserCredentialFingerprint: string;
};

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

function unavailable(): never {
  throw new Error(INTENT_UNAVAILABLE_MESSAGE);
}

function canonicalUuid(value: string): boolean {
  return value !== NIL_UUID && CANONICAL_UUID_PATTERN.test(value);
}

function validStoredIntent(value: unknown): value is StoredRoomAdmissionIntent {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const source = value as Record<string, unknown>;
  const keys = Object.keys(source).sort().join("\n");
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
    source.version === 1 &&
    source.participantType === "human" &&
    typeof source.requestId === "string" &&
    canonicalUuid(source.requestId) &&
    typeof source.inviteCredentialFingerprint === "string" &&
    SHA256_HEX_PATTERN.test(source.inviteCredentialFingerprint) &&
    typeof source.browserCredentialFingerprint === "string" &&
    SHA256_HEX_PATTERN.test(source.browserCredentialFingerprint) &&
    typeof source.meetingId === "string" &&
    source.meetingId.length > 0 &&
    source.meetingId.length <= 128 &&
    typeof source.displayName === "string" &&
    source.displayName.trim().length > 0 &&
    source.displayName.length <= 128 &&
    typeof source.avatarImage === "string" &&
    source.avatarImage.length <= 2048 &&
    typeof source.clientId === "string" &&
    source.clientId.trim() === source.clientId &&
    source.clientId.length > 0 &&
    source.clientId.length <= 128
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

function removeStoredIntent(storage: Storage): boolean {
  storage.removeItem(ROOM_ADMISSION_INTENT_STORAGE_KEY);
  return storage.getItem(ROOM_ADMISSION_INTENT_STORAGE_KEY) === null;
}

function completedSessionContext(
  context: RoomAdmissionIntentContext,
  session: CompletedAdmissionSession | null | undefined
): RoomAdmissionIntentContext | undefined {
  if (
    !session ||
    roomGuestSessionExpired(session) ||
    !session.inviteToken ||
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
): Promise<RoomAdmissionIntent | null> {
  const storage = sessionStorageOwner();
  const stored = readStoredIntent(storage);
  if (!stored) return null;
  const fingerprints = await expectedFingerprints(context);
  if (!storedIntentMatches(stored, context, fingerprints)) {
    const completed = completedSessionContext(context, completedSession);
    if (completed) {
      const completedFingerprints = await expectedFingerprints(completed);
      if (storedIntentMatches(stored, completed, completedFingerprints)) {
        try {
          if (!removeStoredIntent(storage)) unavailable();
          return null;
        } catch {
          unavailable();
        }
      }
    }
    unavailable();
  }
  const {
    version: _version,
    inviteCredentialFingerprint: _inviteFingerprint,
    browserCredentialFingerprint: _browserFingerprint,
    ...intent
  } = stored;
  return intent;
}

export async function loadOrCreateRoomAdmissionIntent(
  input: NewRoomAdmissionIntent
): Promise<RoomAdmissionIntent> {
  const existing = await loadRoomAdmissionIntent(input);
  if (existing) return existing;
  if (typeof globalThis.crypto?.randomUUID !== "function") unavailable();
  const fingerprints = await expectedFingerprints(input);
  const stored: StoredRoomAdmissionIntent = {
    version: 1,
    ...fingerprints,
    requestId: globalThis.crypto.randomUUID(),
    meetingId: input.meetingId,
    displayName: input.displayName,
    avatarImage: input.avatarImage,
    clientId: input.clientId,
    participantType: "human",
  };
  if (!validStoredIntent(stored)) unavailable();
  const serialized = JSON.stringify(stored);
  const storage = sessionStorageOwner();
  try {
    storage.setItem(ROOM_ADMISSION_INTENT_STORAGE_KEY, serialized);
    if (storage.getItem(ROOM_ADMISSION_INTENT_STORAGE_KEY) !== serialized) unavailable();
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

export function clearRoomAdmissionIntent(): boolean {
  try {
    return removeStoredIntent(window.sessionStorage);
  } catch {
    // Verified room-session custody is authoritative after admission completes.
    return false;
  }
}

export function roomAdmissionFailureEndsIntentCustody(error: unknown): boolean {
  return error instanceof ApiError && DEFINITIVE_INTENT_END_CODES.has(error.code);
}
