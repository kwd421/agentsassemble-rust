const CHALLENGE_CONTEXT = "agentsassemble-host-challenge-v1\0";
const REQUEST_CONTEXT = "agentsassemble-host-ticket-request-v1\0";
const RESPONSE_CONTEXT = "agentsassemble-host-ticket-response-v1\0";
const HEX_32_BYTES = /^[0-9a-f]{64}$/;

export interface HostTicketGrant {
  ticket: string;
  ttl_seconds: number;
  server_proof_key: string;
  host_response_proof: string;
}

export interface HostChallengeGrant {
  challenge: string;
  host_challenge_proof: string;
}

export async function verifyHostChallenge(
  hostToken: string,
  grant: HostChallengeGrant
): Promise<boolean> {
  if (
    !HEX_32_BYTES.test(grant.challenge) ||
    !HEX_32_BYTES.test(grant.host_challenge_proof)
  ) {
    return false;
  }
  const key = await importHmacKey(hostToken, ["verify"]);
  return crypto.subtle.verify(
    "HMAC",
    key,
    hexBytes(grant.host_challenge_proof),
    encodeFields(CHALLENGE_CONTEXT, [grant.challenge])
  );
}

export async function signHostTicketRequest(
  hostToken: string,
  challenge: string,
  meetingId: string
): Promise<string> {
  return hmacHex(hostToken, encodeFields(REQUEST_CONTEXT, [challenge, meetingId]));
}

export async function verifyHostTicketResponse(
  hostToken: string,
  challenge: string,
  grant: HostTicketGrant
): Promise<boolean> {
  if (
    !HEX_32_BYTES.test(challenge) ||
    !HEX_32_BYTES.test(grant.ticket) ||
    !HEX_32_BYTES.test(grant.server_proof_key) ||
    !HEX_32_BYTES.test(grant.host_response_proof) ||
    !Number.isSafeInteger(grant.ttl_seconds) ||
    grant.ttl_seconds <= 0
  ) {
    return false;
  }
  const key = await importHmacKey(hostToken, ["verify"]);
  const signature = hexBytes(grant.host_response_proof);
  const payload = encodeFields(RESPONSE_CONTEXT, [
    challenge,
    grant.ticket,
    String(grant.ttl_seconds),
    grant.server_proof_key,
  ]);
  return crypto.subtle.verify("HMAC", key, signature, payload);
}

async function hmacHex(keyText: string, payload: BufferSource): Promise<string> {
  const key = await importHmacKey(keyText, ["sign"]);
  const signature = new Uint8Array(await crypto.subtle.sign("HMAC", key, payload));
  return Array.from(signature, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function importHmacKey(keyText: string, usages: KeyUsage[]): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(keyText),
    { name: "HMAC", hash: "SHA-256" },
    false,
    usages
  );
}

function encodeFields(context: string, fields: string[]): Uint8Array<ArrayBuffer> {
  return new TextEncoder().encode(`${context}${fields.join("\0")}\0`);
}

function hexBytes(value: string): Uint8Array<ArrayBuffer> {
  return Uint8Array.from(value.match(/../g) || [], (byte) => Number.parseInt(byte, 16));
}
