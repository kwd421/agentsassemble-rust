import { isUnicodeScalarString } from "./unicodeScalarString";

function isRustWhitespace(codePoint: number): boolean {
  // Rust char::is_whitespace follows Unicode White_Space; ECMAScript trim does not.
  return (
    (codePoint >= 0x0009 && codePoint <= 0x000d) ||
    codePoint === 0x0020 ||
    codePoint === 0x0085 ||
    codePoint === 0x00a0 ||
    codePoint === 0x1680 ||
    (codePoint >= 0x2000 && codePoint <= 0x200a) ||
    codePoint === 0x2028 ||
    codePoint === 0x2029 ||
    codePoint === 0x202f ||
    codePoint === 0x205f ||
    codePoint === 0x3000
  );
}

function trimRustWhitespace(scalars: string[]): string[] {
  let start = 0;
  let end = scalars.length;
  while (start < end && isRustWhitespace(scalars[start].codePointAt(0)!)) start += 1;
  while (end > start && isRustWhitespace(scalars[end - 1].codePointAt(0)!)) end -= 1;
  return scalars.slice(start, end);
}

export function canonicalRoomId(value: string): string {
  if (!isUnicodeScalarString(value)) {
    throw new Error("방 식별자가 정규 형식이 아닙니다.");
  }
  const scalars = [...value.replace(/[\r\n]/g, " ")];
  const normalized = trimRustWhitespace(
    trimRustWhitespace(scalars).slice(0, 128)
  ).join("");
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
