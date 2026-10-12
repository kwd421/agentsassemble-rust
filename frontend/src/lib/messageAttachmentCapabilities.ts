import { MAX_MESSAGE_ATTACHMENTS_PER_EVENT } from "../types/generated/MESSAGE_ATTACHMENTS_WIRE.ts";

export const LEGACY_MESSAGE_ATTACHMENT_LIMIT = 8;

// Absence belongs to deployed eight-file hosts; malformed advertisements fail closed.
export function parseMessageAttachmentLimit(value: string | null): number {
  if (value === null) return LEGACY_MESSAGE_ATTACHMENT_LIMIT;
  const limit = Number(value);
  if (!/^[1-9][0-9]*$/.test(value) || !Number.isSafeInteger(limit) || limit > MAX_MESSAGE_ATTACHMENTS_PER_EVENT) {
    throw new Error("Invalid host attachment limit.");
  }
  return limit;
}
