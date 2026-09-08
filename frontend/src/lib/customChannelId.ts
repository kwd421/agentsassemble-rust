export function isCustomChannelId(value: unknown): value is string {
  return typeof value === "string" && /^c[0-9a-f]{12}$/.test(value);
}
