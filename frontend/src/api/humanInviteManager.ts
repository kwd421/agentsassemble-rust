import type { RoomAppearance } from "../lib/roomAppearance";
import { decodeCanonicalBase64Url } from "../lib/base64Url";
import {
  fetchDesktopHumanInviteCreate,
  fetchDesktopHumanInviteRevoke,
  parseDesktopLoopbackHttpBase,
  parseDesktopManagerRoomAuthority,
  type DesktopManagerRoomAuthority,
} from "../lib/desktopBridge";
import { sha256Hex, utf8 } from "../lib/lengthDelimitedCrypto";
import { parsePublicIngressOrigin } from "../lib/publicIngressStatus";
import {
  HUMAN_INVITE_JOIN_CODE_BYTES,
  HUMAN_INVITE_JOIN_CODE_PREFIX,
  HUMAN_INVITE_SIGNATURE_BYTES,
  HUMAN_INVITE_SIGNED_TOKEN_MAX_BYTES,
  HUMAN_INVITE_SIGNED_TOKEN_PREFIX,
} from "../types/generated/HUMAN_INVITE_WIRE";

export type HumanInviteDispatchOutcome =
  | "proven_not_dispatched"
  | "outcome_unknown";

export class HumanInviteDispatchError extends Error {
  readonly outcome: HumanInviteDispatchOutcome;

  constructor(outcome: HumanInviteDispatchOutcome) {
    super(
      outcome === "proven_not_dispatched"
        ? "사람 초대 요청이 전송되지 않았습니다."
        : "사람 초대 요청 결과를 확인할 수 없습니다."
    );
    this.name = "HumanInviteDispatchError";
    this.outcome = outcome;
  }
}

export type ManagedHumanInviteCreateIntent = {
  authority: DesktopManagerRoomAuthority;
  displayName: string;
  inviteScope: RoomAppearance["inviteScope"];
  ttlSeconds: number;
  maxUses: number;
};

export type ManagedHumanInviteCustody = Readonly<{
  authority: Readonly<DesktopManagerRoomAuthority>;
  inviteId: string;
  joinUrl: string;
  responseOrigin: string;
  expiresAt: Readonly<{
    exact: string;
    epochMilliseconds: number;
  }>;
}>;

export type ManagedHumanInviteRevokeResult = "revoked" | "invite_not_found";

const CREATE_RESPONSE_KEYS = [
  "invite_id",
  "invite_token",
  "join_code",
  "meeting_id",
  "agent_id",
  "display_name",
  "invite_scope",
  "participant_type",
  "client_type",
  "provider_kind",
  "permission_mode",
  "max_uses",
  "expires_at",
  "room_url",
  "join_url",
] as const;

const CANONICAL_TIMESTAMP =
  /^(\d{4}|[+-]\d{4,6})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d{6}))?\+00:00$/;

function invalidResponse(): never {
  throw new Error("사람 초대 응답 계약이 올바르지 않습니다.");
}

function exactObject(
  value: unknown,
  keys: readonly string[]
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) invalidResponse();
  const record = value as Record<string, unknown>;
  const actual = Object.keys(record).sort();
  const expected = [...keys].sort();
  if (
    actual.length !== expected.length ||
    actual.some((key, index) => key !== expected[index])
  ) {
    invalidResponse();
  }
  return record;
}

function exactString(value: unknown): string {
  if (typeof value !== "string") invalidResponse();
  return value;
}

function parseServerExpiry(value: unknown) {
  const exact = exactString(value);
  const match = CANONICAL_TIMESTAMP.exec(exact);
  if (!match) invalidResponse();
  const [yearText, monthText, dayText, hourText, minuteText, secondText, microsText] =
    match.slice(1);
  const year = Number(yearText);
  const canonicalYear =
    year < 0
      ? `-${String(-year).padStart(4, "0")}`
      : year > 9999
        ? `+${year}`
        : String(year).padStart(4, "0");
  const micros = Number(microsText || 0);
  if (yearText !== canonicalYear || (microsText !== undefined && micros === 0)) {
    invalidResponse();
  }
  const components = [
    year,
    Number(monthText),
    Number(dayText),
    Number(hourText),
    Number(minuteText),
    Number(secondText),
    Math.floor(micros / 1000),
  ];
  const parsed = new Date(0);
  parsed.setUTCFullYear(components[0], components[1] - 1, components[2]);
  parsed.setUTCHours(components[3], components[4], components[5], components[6]);
  const epochMilliseconds = parsed.getTime();
  if (
    !Number.isFinite(epochMilliseconds) ||
    parsed.getUTCFullYear() !== components[0] ||
    parsed.getUTCMonth() + 1 !== components[1] ||
    parsed.getUTCDate() !== components[2] ||
    parsed.getUTCHours() !== components[3] ||
    parsed.getUTCMinutes() !== components[4] ||
    parsed.getUTCSeconds() !== components[5] ||
    parsed.getUTCMilliseconds() !== components[6]
  ) {
    invalidResponse();
  }
  return Object.freeze({ exact, epochMilliseconds });
}

function parseJoinUrl(value: unknown, joinCode: string) {
  const exact = exactString(value);
  let url: URL;
  try {
    url = new URL(exact);
    parsePublicIngressOrigin(url.origin);
  } catch {
    invalidResponse();
  }
  if (
    exact !== url.toString() ||
    url.pathname !== "/join" ||
    url.username ||
    url.password ||
    url.hash ||
    url.search !== `?token=${joinCode}`
  ) {
    invalidResponse();
  }
  return { exact, origin: url.origin };
}

function isCanonicalSignedInviteToken(value: string): boolean {
  if (value.length > HUMAN_INVITE_SIGNED_TOKEN_MAX_BYTES) return false;
  const segments = value.split(".");
  if (segments.length !== 3 || segments[0] !== HUMAN_INVITE_SIGNED_TOKEN_PREFIX) {
    return false;
  }
  const payload = decodeCanonicalBase64Url(segments[1]);
  const signature = decodeCanonicalBase64Url(segments[2]);
  return payload !== null && signature?.length === HUMAN_INVITE_SIGNATURE_BYTES;
}

function isCanonicalJoinCode(value: string): boolean {
  if (!value.startsWith(HUMAN_INVITE_JOIN_CODE_PREFIX)) return false;
  const encoded = value.slice(HUMAN_INVITE_JOIN_CODE_PREFIX.length);
  if (encoded.length !== (HUMAN_INVITE_JOIN_CODE_BYTES * 4) / 3) return false;
  return (
    decodeCanonicalBase64Url(encoded)?.length === HUMAN_INVITE_JOIN_CODE_BYTES
  );
}

function validateCreateIntent(intent: ManagedHumanInviteCreateIntent) {
  const authority = parseDesktopManagerRoomAuthority(intent.authority);
  if (typeof intent.displayName !== "string") {
    throw new Error("사람 초대 요청 계약이 올바르지 않습니다.");
  }
  const displayCharacters = Array.from(intent.displayName);
  if (
    !intent.displayName ||
    intent.displayName.trim() !== intent.displayName ||
    intent.displayName.split(/\s+/u).join(" ") !== intent.displayName ||
    displayCharacters.length > 128 ||
    displayCharacters.some((character) => /\p{Cc}/u.test(character)) ||
    (intent.inviteScope !== "room" && intent.inviteScope !== "read_only") ||
    !Number.isSafeInteger(intent.ttlSeconds) ||
    intent.ttlSeconds < 1 ||
    !Number.isSafeInteger(intent.maxUses) ||
    intent.maxUses < 0
  ) {
    throw new Error("사람 초대 요청 계약이 올바르지 않습니다.");
  }
  return {
    authority,
    displayName: intent.displayName,
    inviteScope: intent.inviteScope,
    ttlSeconds: intent.ttlSeconds,
    maxUses: intent.maxUses,
  };
}

export async function parseManagedHumanInviteCreateResponse(
  value: unknown,
  intent: ManagedHumanInviteCreateIntent
): Promise<ManagedHumanInviteCustody> {
  const request = validateCreateIntent(intent);
  const response = exactObject(value, CREATE_RESPONSE_KEYS);
  const inviteToken = exactString(response.invite_token);
  const joinCode = exactString(response.join_code);
  const inviteId = exactString(response.invite_id);
  if (
    !isCanonicalSignedInviteToken(inviteToken) ||
    !isCanonicalJoinCode(joinCode) ||
    !/^[0-9a-f]{16}$/.test(inviteId) ||
    inviteId !== (await sha256Hex(utf8(inviteToken))).slice(0, 16) ||
    response.meeting_id !== request.authority.room_id ||
    response.display_name !== request.displayName ||
    response.invite_scope !== request.inviteScope ||
    response.participant_type !== "human" ||
    response.client_type !== "browser" ||
    response.provider_kind !== "manual" ||
    response.permission_mode !==
      (request.inviteScope === "read_only" ? "meeting_read_only" : "participant") ||
    response.max_uses !== request.maxUses ||
    !/^guest-[0-9a-f]{32}$/.test(exactString(response.agent_id))
  ) {
    invalidResponse();
  }
  try {
    parseDesktopLoopbackHttpBase(exactString(response.room_url));
  } catch {
    invalidResponse();
  }
  const join = parseJoinUrl(response.join_url, joinCode);
  return Object.freeze({
    authority: request.authority,
    inviteId,
    joinUrl: join.exact,
    responseOrigin: join.origin,
    expiresAt: parseServerExpiry(response.expires_at),
  });
}

function dispatchedMarker(
  beforeDispatch: (() => void) | undefined,
  markDispatched: () => void
) {
  return () => {
    beforeDispatch?.();
    markDispatched();
  };
}

function dispatchError(dispatched: boolean): HumanInviteDispatchError {
  return new HumanInviteDispatchError(
    dispatched ? "outcome_unknown" : "proven_not_dispatched"
  );
}

export async function createManagedHumanInvite(
  intent: ManagedHumanInviteCreateIntent,
  beforeDispatch?: () => void
): Promise<ManagedHumanInviteCustody> {
  let dispatched = false;
  try {
    const request = validateCreateIntent(intent);
    const response = await fetchDesktopHumanInviteCreate(
      request.authority,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          meeting_id: request.authority.room_id,
          display_name: request.displayName,
          invite_scope: request.inviteScope,
          ttl_seconds: request.ttlSeconds,
          max_uses: request.maxUses,
        }),
      },
      dispatchedMarker(beforeDispatch, () => {
        dispatched = true;
      })
    );
    if (response.status !== 200) invalidResponse();
    return await parseManagedHumanInviteCreateResponse(await response.json(), request);
  } catch {
    throw dispatchError(dispatched);
  }
}

function exactInviteNotFound(value: unknown): boolean {
  try {
    const envelope = exactObject(value, ["error"]);
    const error = exactObject(envelope.error, ["code", "message"]);
    return error.code === "invite_not_found" && Boolean(exactString(error.message).trim());
  } catch {
    return false;
  }
}

export async function revokeManagedHumanInvite(
  custody: Pick<ManagedHumanInviteCustody, "authority" | "inviteId">,
  beforeDispatch?: () => void
): Promise<ManagedHumanInviteRevokeResult> {
  let dispatched = false;
  try {
    const authority = parseDesktopManagerRoomAuthority(custody.authority);
    if (!/^[0-9a-f]{16}$/.test(custody.inviteId)) {
      throw new Error("사람 초대 취소 계약이 올바르지 않습니다.");
    }
    const response = await fetchDesktopHumanInviteRevoke(
      authority,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          meeting_id: authority.room_id,
          invite_id: custody.inviteId,
        }),
      },
      dispatchedMarker(beforeDispatch, () => {
        dispatched = true;
      })
    );
    const payload = await response.json();
    if (response.status === 404 && exactInviteNotFound(payload)) {
      return "invite_not_found";
    }
    const result = exactObject(payload, ["status", "invite_id"]);
    if (
      response.status !== 200 ||
      result.status !== "revoked" ||
      result.invite_id !== custody.inviteId
    ) {
      invalidResponse();
    }
    return "revoked";
  } catch {
    throw dispatchError(dispatched);
  }
}
