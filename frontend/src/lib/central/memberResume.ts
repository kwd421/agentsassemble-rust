// The member resume protocol: the bytes the client signs and the answers it
// accepts. The host rebuilds the signed bytes from the request it receives,
// so a host-supplied blob is never what gets signed.
import { encodeBase64Url } from "../base64Url";
import { assertExactKeys, requiredString, strictRecord } from "../strictJsonContract";

/** The domain separator both sides sign. */
export const MEMBER_RESUME_PROTOCOL = "AA-MEMBER-RESUME-1";
/** The purpose both sides sign. */
export const MEMBER_RESUME_PURPOSE = "resume";

export type MemberResumeTuple = {
  server_id: string;
  registration_epoch: string;
  origin: string;
  generation: number;
  host_key_fingerprint: string;
  browser_credential_fingerprint: string;
  client_public_key: string;
  channel_id: string;
  nonce: string;
};

export type MemberResumeRoom = { room_id: string; name: string };

/** The exact bytes the client signs over a resume attempt. */
export function memberResumeTranscript(tuple: MemberResumeTuple): Uint8Array<ArrayBuffer> {
  return new TextEncoder().encode(JSON.stringify([
    MEMBER_RESUME_PROTOCOL,
    tuple.server_id,
    tuple.registration_epoch,
    tuple.origin,
    tuple.generation,
    tuple.host_key_fingerprint,
    tuple.browser_credential_fingerprint,
    tuple.client_public_key,
    tuple.channel_id,
    MEMBER_RESUME_PURPOSE,
    tuple.nonce,
  ]));
}

/** Signs the resume transcript with the device's own resume key. */
export async function signMemberResume(
  key: CryptoKey,
  transcript: Uint8Array<ArrayBuffer>,
): Promise<string> {
  const signature = await crypto.subtle.sign(
    { name: "ECDSA", hash: "SHA-256" },
    key,
    transcript,
  );
  return encodeBase64Url(signature);
}

/** The fingerprint the host compares: the SHA-256 of the browser credential. */
export async function browserCredentialFingerprint(credential: string): Promise<string> {
  return encodeBase64Url(
    await crypto.subtle.digest("SHA-256", new TextEncoder().encode(credential)),
  );
}

export function parseMemberResumeNonce(value: unknown): { nonce: string; expires_at: number } {
  const record = strictRecord(value, "재연결 확인");
  assertExactKeys(record, ["nonce", "expires_at"], "재연결 확인");
  const nonce = requiredString(record, "nonce", "재연결 확인");
  const expires_at = record.expires_at;
  if (
    !nonce ||
    nonce.length > 200 ||
    typeof expires_at !== "number" ||
    !Number.isSafeInteger(expires_at) ||
    expires_at <= Math.floor(Date.now() / 1000)
  ) {
    throw new Error("재연결 확인을 읽지 못했어요.");
  }
  return { nonce, expires_at };
}

export function parseMemberResumeAnswer(value: unknown): {
  rooms: MemberResumeRoom[];
  challenge_id: string;
  challenge_expires_at: number;
} {
  const record = strictRecord(value, "참가 중인 방");
  assertExactKeys(record, ["rooms", "challenge_id", "challenge_expires_at"], "참가 중인 방");
  const rooms = record.rooms;
  if (
    !Array.isArray(rooms) ||
    rooms.length > 50 ||
    !rooms.every(
      (room) =>
        room &&
        typeof room === "object" &&
        typeof (room as { room_id?: unknown }).room_id === "string" &&
        (room as { room_id: string }).room_id !== "" &&
        typeof (room as { name?: unknown }).name === "string",
    )
  ) {
    throw new Error("참가 중인 방을 확인하지 못했어요.");
  }
  const challenge_id = requiredString(record, "challenge_id", "참가 중인 방");
  const challenge_expires_at = record.challenge_expires_at;
  if (
    !challenge_id ||
    challenge_id.length > 200 ||
    typeof challenge_expires_at !== "number" ||
    !Number.isSafeInteger(challenge_expires_at)
  ) {
    throw new Error("참가 중인 방을 확인하지 못했어요.");
  }
  return {
    rooms: rooms as MemberResumeRoom[],
    challenge_id,
    challenge_expires_at,
  };
}
