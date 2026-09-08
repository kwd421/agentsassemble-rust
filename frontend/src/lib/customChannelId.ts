export function isCustomChannelId(value: unknown): value is string {
  return typeof value === "string" && /^c[0-9a-f]{12}$/.test(value);
}

export function requireMessageChannelId(value: unknown): string {
  if (value === "lobby" || isCustomChannelId(value)) return value;
  throw new Error("메시지 채널 식별자가 올바르지 않습니다.");
}
