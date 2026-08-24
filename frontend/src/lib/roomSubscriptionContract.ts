import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import type { Subscribed } from "../types/generated/Subscribed";
import {
  deriveConnectionNonce,
  digestPermissions,
  digestSnapshotFrame,
  isHex32Bytes,
  verifySubscriptionProof,
} from "./serverProof";

const RECEIPT_KEYS = [
  "op",
  "streams",
  "protocol_version",
  "server_challenge",
  "connection_nonce",
  "room_id",
  "principal_id",
  "participant_id",
  "server_surface_revision",
  "server_surface_digest",
  "permissions_digest",
  "snapshot_cursor",
  "catchup_high_water",
  "snapshot_digest",
  "proof",
] as const;

export type SubscriptionReceipt = Subscribed & { op: "subscribed" };

export class SubscriptionContractError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "SubscriptionContractError";
    this.code = code;
  }
}

type ExpectedSubscription = {
  ticket: string;
  proofKey: string;
  serverChallenge: string;
  roomId: string;
  participantId: string;
  streams: readonly string[];
  serverSurface: ServerProductSurface;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function isSequence(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function exactKeys(value: Record<string, unknown>): boolean {
  const actual = Object.keys(value).sort();
  const expected = [...RECEIPT_KEYS].sort();
  return actual.length === expected.length && actual.every((key, index) => key === expected[index]);
}

export async function verifySubscriptionReceipt(
  value: unknown,
  expected: ExpectedSubscription
): Promise<SubscriptionReceipt> {
  if (!isRecord(value) || !exactKeys(value)) {
    throw new SubscriptionContractError(
      "subscription_receipt_schema_invalid",
      "The runtime returned an invalid subscription receipt."
    );
  }
  const receipt = value as unknown as SubscriptionReceipt;
  const expectedStreams = [...expected.streams].sort();
  const receivedStreams = Array.isArray(receipt.streams) ? [...receipt.streams].sort() : [];
  const schemaValid =
    receipt.op === "subscribed" &&
    receipt.protocol_version === 1 &&
    receivedStreams.length === expectedStreams.length &&
    receivedStreams.every((stream, index) => stream === expectedStreams[index]) &&
    receipt.server_challenge === expected.serverChallenge &&
    typeof receipt.room_id === "string" &&
    receipt.room_id === expected.roomId &&
    typeof receipt.principal_id === "string" &&
    Boolean(receipt.principal_id) &&
    receipt.participant_id === expected.participantId &&
    receipt.server_surface_revision === expected.serverSurface.revision &&
    receipt.server_surface_digest === expected.serverSurface.digest &&
    isHex32Bytes(receipt.server_surface_digest) &&
    isHex32Bytes(receipt.permissions_digest) &&
    isHex32Bytes(receipt.snapshot_digest) &&
    isHex32Bytes(receipt.proof) &&
    isSequence(receipt.snapshot_cursor) &&
    isSequence(receipt.catchup_high_water) &&
    receipt.catchup_high_water >= receipt.snapshot_cursor;
  if (!schemaValid) {
    throw new SubscriptionContractError(
      "subscription_receipt_scope_invalid",
      "The runtime subscription receipt did not match the requested room authority."
    );
  }
  const connectionNonce = await deriveConnectionNonce(expected.ticket);
  if (
    receipt.connection_nonce !== connectionNonce ||
    !(await verifySubscriptionProof(expected.proofKey, receipt))
  ) {
    throw new SubscriptionContractError(
      "server_identity_invalid",
      "The desktop runtime did not prove the complete subscription boundary."
    );
  }
  return receipt;
}

export async function verifyBoundSnapshot(
  raw: string,
  value: unknown,
  receipt: SubscriptionReceipt
): Promise<void> {
  if (!isRecord(value) || value.op !== "snapshot" || value.last_seq !== receipt.snapshot_cursor) {
    throw new SubscriptionContractError(
      "snapshot_boundary_invalid",
      "The room snapshot did not match its authenticated cursor."
    );
  }
  const [actualSnapshotDigest, actualPermissionsDigest] = await Promise.all([
    digestSnapshotFrame(raw),
    digestPermissions(value.capabilities),
  ]);
  if (
    actualSnapshotDigest !== receipt.snapshot_digest ||
    actualPermissionsDigest !== receipt.permissions_digest
  ) {
    throw new SubscriptionContractError(
      "snapshot_binding_invalid",
      "The room snapshot did not match its authenticated bytes and permissions."
    );
  }
}
