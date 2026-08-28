export function canonicalRoomId(value: string): string {
  const normalized = [...value.replace(/[\r\n]/g, " ").trim()]
    .slice(0, 128)
    .join("")
    .trim();
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
