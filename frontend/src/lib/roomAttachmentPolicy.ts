import { ROOM_FILE_LIMITS, ROOM_STORAGE_LIMITS } from "../types/generated/ASSET_SAFETY_WIRE";

export function roomAttachmentPolicyIsValid(value: {
  attachment_file_limit_bytes?: unknown;
  attachment_storage_limit_bytes?: unknown;
}): boolean {
  return (value.attachment_file_limit_bytes === undefined ||
      (ROOM_FILE_LIMITS as readonly unknown[]).includes(value.attachment_file_limit_bytes)) &&
    (value.attachment_storage_limit_bytes === undefined || value.attachment_storage_limit_bytes === null ||
      (ROOM_STORAGE_LIMITS as readonly unknown[]).includes(value.attachment_storage_limit_bytes));
}
