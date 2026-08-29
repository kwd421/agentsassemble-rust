import {
  MESSAGE_ATTACHMENT_ID_HEX_LENGTH,
  MESSAGE_ATTACHMENT_ID_PREFIX,
  MESSAGE_ATTACHMENT_DOWNLOAD_SUFFIX,
  MESSAGE_ATTACHMENT_REFERENCE_PREFIX,
  MESSAGE_ATTACHMENT_VIEW_SUFFIX,
} from "../types/generated/MESSAGE_ATTACHMENTS_WIRE";

export function messageAttachmentId(value: string): string {
  const hex = value.startsWith(MESSAGE_ATTACHMENT_ID_PREFIX)
    ? value.slice(MESSAGE_ATTACHMENT_ID_PREFIX.length)
    : "";
  if (
    hex.length !== MESSAGE_ATTACHMENT_ID_HEX_LENGTH ||
    ![...hex].every(
      (character) =>
        (character >= "0" && character <= "9") ||
        (character >= "a" && character <= "f")
    )
  ) {
    throw new Error("메시지 첨부 식별자가 올바르지 않습니다.");
  }
  return value;
}

export function messageAttachmentReference(
  attachmentId: string,
  mode: "view" | "download"
): string {
  const id = messageAttachmentId(attachmentId);
  const suffix =
    mode === "view"
      ? MESSAGE_ATTACHMENT_VIEW_SUFFIX
      : MESSAGE_ATTACHMENT_DOWNLOAD_SUFFIX;
  return `${MESSAGE_ATTACHMENT_REFERENCE_PREFIX}${id}${suffix}`;
}
