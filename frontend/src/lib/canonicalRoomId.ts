import { isUnicodeScalarString } from "./unicodeScalarString";
import { trimRustWhitespace } from "./rustWhitespace";

export function canonicalRoomId(value: string): string {
  if (!isUnicodeScalarString(value)) {
    throw new Error("방 식별자가 정규 형식이 아닙니다.");
  }
  const trimmed = trimRustWhitespace(value.replace(/[\r\n]/g, " "));
  const normalized = trimRustWhitespace([...trimmed].slice(0, 128).join(""));
  if (
    !normalized ||
    normalized !== value ||
    normalized === "." ||
    normalized === ".." ||
    normalized.includes("/") ||
    normalized.includes("\\")
  ) {
    throw new Error("방 식별자가 정규 형식이 아닙니다.");
  }
  return normalized;
}
