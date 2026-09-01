import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import type { Subscribed } from "../types/generated/Subscribed";

const SHA256_HEX = /^[0-9a-f]{64}$/;

const RECEIPT_KEYS = [
  "op",
  "streams",
  "protocol_version",
  "room_id",
  "principal_id",
  "participant_id",
  "server_surface_revision",
  "server_surface_digest",
  "snapshot_cursor",
  "catchup_high_water",
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

export function isSha256Hex(value: unknown): value is string {
  return typeof value === "string" && SHA256_HEX.test(value);
}

export function verifySubscriptionReceipt(
  value: unknown,
  expected: ExpectedSubscription
): SubscriptionReceipt {
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
    typeof receipt.room_id === "string" &&
    receipt.room_id === expected.roomId &&
    typeof receipt.principal_id === "string" &&
    Boolean(receipt.principal_id) &&
    receipt.participant_id === expected.participantId &&
    receipt.server_surface_revision === expected.serverSurface.revision &&
    receipt.server_surface_digest === expected.serverSurface.digest &&
    isSha256Hex(receipt.server_surface_digest) &&
    isSequence(receipt.snapshot_cursor) &&
    isSequence(receipt.catchup_high_water) &&
    receipt.catchup_high_water >= receipt.snapshot_cursor;
  if (!schemaValid) {
    throw new SubscriptionContractError(
      "subscription_receipt_scope_invalid",
      "The runtime subscription receipt did not match the requested room authority."
    );
  }
  return receipt;
}

export function verifyBoundSnapshot(
  value: unknown,
  receipt: SubscriptionReceipt
): void {
  if (!isRecord(value) || value.op !== "snapshot" || value.last_seq !== receipt.snapshot_cursor) {
    throw new SubscriptionContractError(
      "snapshot_boundary_invalid",
      "The room snapshot did not match its subscription cursor."
    );
  }
}
