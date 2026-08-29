import {
  MESSAGE_ATTACHMENT_ID_HEX_LENGTH,
  MESSAGE_ATTACHMENT_ID_PREFIX,
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
