export function strictRecord(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} 응답 형식이 올바르지 않습니다.`);
  }
  return value as Record<string, unknown>;
}

export function assertExactKeys(
  value: Record<string, unknown>,
  required: readonly string[],
  label: string,
  optional: readonly string[] = []
) {
  const actual = Object.keys(value);
  const allowed = new Set([...required, ...optional]);
  if (
    required.some((key) => !Object.hasOwn(value, key)) ||
    actual.some((key) => !allowed.has(key))
  ) {
    throw new Error(`${label} 응답 계약이 일치하지 않습니다.`);
  }
}

export function requiredString(
  value: Record<string, unknown>,
  key: string,
  label: string
): string {
  if (typeof value[key] !== "string" || !value[key]) {
    throw new Error(`${label}.${key}가 올바르지 않습니다.`);
  }
  return value[key];
}

export function optionalString(
  value: Record<string, unknown>,
  key: string,
  label: string
): string | undefined {
  const field = value[key];
  if (field === undefined) return undefined;
  if (typeof field !== "string") {
    throw new Error(`${label}.${key}가 올바르지 않습니다.`);
  }
  return field || undefined;
}
