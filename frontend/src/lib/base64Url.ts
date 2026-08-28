export function encodeBase64Url(value: ArrayBuffer | Uint8Array): string {
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

export function isBase64UrlText(value: string): boolean {
  return Boolean(
    value && /^[A-Za-z0-9_-]+$/.test(value) && value.length % 4 !== 1
  );
}

export function decodeCanonicalBase64Url(
  value: string
): Uint8Array<ArrayBuffer> | null {
  if (!isBase64UrlText(value)) return null;
  try {
    const padded = value.replace(/-/g, "+").replace(/_/g, "/").padEnd(
      value.length + ((4 - (value.length % 4)) % 4),
      "="
    );
    const binary = atob(padded);
    const bytes = Uint8Array.from(
      binary,
      (character) => character.charCodeAt(0)
    );
    return encodeBase64Url(bytes) === value ? bytes : null;
  } catch {
    return null;
  }
}
