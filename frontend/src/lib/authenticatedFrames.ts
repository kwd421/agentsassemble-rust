import {
  lengthDelimitedByteTranscript,
  lengthDelimitedTranscript,
  utf8,
} from "./lengthDelimitedCrypto";
import { isHex32Bytes } from "./serverProof";

const FRAME_KEY_CONTEXT = "agentsassemble.ws-frame-key.v1";
const FRAME_PROOF_CONTEXT = "agentsassemble.ws-frame-proof.v1";
const MAX_INNER_MESSAGE_BYTES = 256 * 1024;
const MAX_BASE64_PAYLOAD_BYTES = Math.ceil(MAX_INNER_MESSAGE_BYTES / 3) * 4;
const ENVELOPE_KEYS = ["counter", "op", "payload", "proof"];
const decoder = new TextDecoder("utf-8", { fatal: true });

export type AuthenticatedFrameDirection = "client" | "server";

export async function deriveAuthenticatedFrameKey(
  proofKey: string,
  connectionNonce: string
): Promise<CryptoKey> {
  if (!isHex32Bytes(proofKey) || !isHex32Bytes(connectionNonce)) {
    throw new Error("Authenticated frame key material is invalid.");
  }
  const rootKey = await crypto.subtle.importKey(
    "raw",
    utf8(proofKey),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const derived = await crypto.subtle.sign(
    "HMAC",
    rootKey,
    lengthDelimitedTranscript(FRAME_KEY_CONTEXT, [connectionNonce])
  );
  return crypto.subtle.importKey(
    "raw",
    derived,
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"]
  );
}

export async function encodeAuthenticatedFrame(
  key: CryptoKey,
  connectionNonce: string,
  direction: AuthenticatedFrameDirection,
  counter: number,
  rawPayload: string
): Promise<string> {
  validateCounter(counter);
  const payload = utf8(rawPayload);
  if (payload.length > MAX_INNER_MESSAGE_BYTES) {
    throw new Error("Authenticated frame payload exceeds the product limit.");
  }
  const proof = await signFrame(key, connectionNonce, direction, counter, payload);
  return JSON.stringify({
    op: "authenticated",
    counter,
    payload: encodeBase64(payload),
    proof,
  });
}

export async function decodeAuthenticatedFrame(
  key: CryptoKey,
  connectionNonce: string,
  direction: AuthenticatedFrameDirection,
  expectedCounter: number,
  rawEnvelope: string
): Promise<string> {
  const envelope = JSON.parse(rawEnvelope) as unknown;
  if (!isExactEnvelope(envelope) || envelope.counter !== expectedCounter) {
    throw new Error("Authenticated frame envelope or counter is invalid.");
  }
  const payload = decodeBase64(envelope.payload);
  if (payload.length > MAX_INNER_MESSAGE_BYTES) {
    throw new Error("Authenticated frame payload exceeds the product limit.");
  }
  const valid = await crypto.subtle.verify(
    "HMAC",
    key,
    hexBytes(envelope.proof),
    frameTranscript(connectionNonce, direction, envelope.counter, payload)
  );
  if (!valid) throw new Error("Authenticated frame proof is invalid.");
  return decoder.decode(payload);
}

async function signFrame(
  key: CryptoKey,
  connectionNonce: string,
  direction: AuthenticatedFrameDirection,
  counter: number,
  payload: Uint8Array<ArrayBuffer>
): Promise<string> {
  const signature = new Uint8Array(
    await crypto.subtle.sign(
      "HMAC",
      key,
      frameTranscript(connectionNonce, direction, counter, payload)
    )
  );
  return Array.from(signature, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function frameTranscript(
  connectionNonce: string,
  direction: AuthenticatedFrameDirection,
  counter: number,
  payload: Uint8Array<ArrayBuffer>
) {
  return lengthDelimitedByteTranscript(FRAME_PROOF_CONTEXT, [
    utf8(connectionNonce),
    utf8(direction),
    utf8(String(counter)),
    payload,
  ]);
}

function isExactEnvelope(value: unknown): value is {
  op: "authenticated";
  counter: number;
  payload: string;
  proof: string;
} {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const envelope = value as Record<string, unknown>;
  const keys = Object.keys(envelope).sort();
  return Boolean(
    keys.length === ENVELOPE_KEYS.length &&
    keys.every((key, index) => key === ENVELOPE_KEYS[index]) &&
    envelope.op === "authenticated" &&
    typeof envelope.counter === "number" &&
    Number.isSafeInteger(envelope.counter) &&
    envelope.counter >= 1 &&
    typeof envelope.payload === "string" &&
    envelope.payload.length <= MAX_BASE64_PAYLOAD_BYTES &&
    typeof envelope.proof === "string" &&
    isHex32Bytes(envelope.proof)
  );
}

function validateCounter(counter: number) {
  if (!Number.isSafeInteger(counter) || counter < 1) {
    throw new Error("Authenticated frame counter is invalid.");
  }
}

function encodeBase64(bytes: Uint8Array<ArrayBuffer>): string {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 8192) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 8192));
  }
  return btoa(binary);
}

function decodeBase64(value: string): Uint8Array<ArrayBuffer> {
  if (value.length > MAX_BASE64_PAYLOAD_BYTES) {
    throw new Error("Authenticated frame payload exceeds the product limit.");
  }
  const binary = atob(value);
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  if (encodeBase64(bytes) !== value) {
    throw new Error("Authenticated frame payload is not canonical base64.");
  }
  return bytes;
}

function hexBytes(value: string): Uint8Array<ArrayBuffer> {
  return Uint8Array.from(value.match(/../g) || [], (byte) => Number.parseInt(byte, 16));
}
