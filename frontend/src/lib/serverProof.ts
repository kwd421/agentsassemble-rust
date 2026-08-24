import type { Subscribed } from "../types/generated/Subscribed";
import {
  lengthDelimitedTranscript,
  sha256Hex,
  utf8,
} from "./lengthDelimitedCrypto";

const CONNECTION_NONCE_CONTEXT = "agentsassemble.ws-connection-nonce.v1";
const PERMISSIONS_CONTEXT = "agentsassemble.permissions.v1";
const SUBSCRIPTION_PROOF_CONTEXT = "agentsassemble.subscription-proof.v1";
const HEX_32_BYTES = /^[0-9a-f]{64}$/;

const CAPABILITY_KEYS = [
  "agent.control",
  "bridge.publish",
  "bridge.report",
  "message.modify",
  "message.send",
  "participant.kick",
  "participant.leave",
  "participant.mute",
  "provider.request.resolve",
  "room.delete",
  "room.history",
  "room.manage",
  "room.random",
  "room.vote.summary",
] as const;

export function createServerChallenge(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function isHex32Bytes(value: unknown): value is string {
  return typeof value === "string" && HEX_32_BYTES.test(value);
}

export async function deriveConnectionNonce(ticket: string): Promise<string> {
  return sha256Hex(lengthDelimitedTranscript(CONNECTION_NONCE_CONTEXT, [ticket]));
}

export async function digestSnapshotFrame(raw: string): Promise<string> {
  return sha256Hex(utf8(raw));
}

export async function digestPermissions(value: unknown): Promise<string | null> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const capabilities = value as Record<string, unknown>;
  const actualKeys = Object.keys(capabilities).sort();
  if (
    actualKeys.length !== CAPABILITY_KEYS.length ||
    actualKeys.some((key, index) => key !== CAPABILITY_KEYS[index]) ||
    CAPABILITY_KEYS.some((key) => typeof capabilities[key] !== "boolean")
  ) {
    return null;
  }
  return sha256Hex(
    lengthDelimitedTranscript(
      PERMISSIONS_CONTEXT,
      CAPABILITY_KEYS.map((key) => `${key}=${capabilities[key] ? 1 : 0}`)
    )
  );
}

export async function verifySubscriptionProof(
  proofKey: string,
  receipt: Subscribed
): Promise<boolean> {
  if (!isHex32Bytes(proofKey) || !isHex32Bytes(receipt.proof)) return false;
  const transcript = subscriptionProofTranscript(receipt);
  const key = await crypto.subtle.importKey(
    "raw",
    utf8(proofKey),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["verify"]
  );
  const signature = Uint8Array.from(
    receipt.proof.match(/../g) || [],
    (byte) => Number.parseInt(byte, 16)
  );
  return crypto.subtle.verify("HMAC", key, signature, transcript);
}

export function subscriptionProofTranscript(receipt: Subscribed): Uint8Array<ArrayBuffer> {
  const streams = [...receipt.streams].sort();
  return lengthDelimitedTranscript(SUBSCRIPTION_PROOF_CONTEXT, [
    receipt.server_challenge,
    receipt.connection_nonce,
    receipt.room_id,
    receipt.principal_id,
    receipt.participant_id,
    String(receipt.protocol_version),
    "streams",
    ...streams,
    String(receipt.server_surface_revision),
    receipt.server_surface_digest,
    receipt.permissions_digest,
    String(receipt.snapshot_cursor),
    String(receipt.catchup_high_water),
    receipt.snapshot_digest,
  ]);
}
