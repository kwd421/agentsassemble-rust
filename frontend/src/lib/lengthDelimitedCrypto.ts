const encoder = new TextEncoder();

export function lengthDelimitedTranscript(
  context: string,
  fields: readonly string[]
): Uint8Array<ArrayBuffer> {
  const values = [context, ...fields].map((value) => encoder.encode(value));
  const size = values.reduce((total, value) => total + 8 + value.length, 0);
  const transcript = new Uint8Array(size);
  const view = new DataView(transcript.buffer);
  let offset = 0;
  for (const value of values) {
    view.setBigUint64(offset, BigInt(value.length), false);
    offset += 8;
    transcript.set(value, offset);
    offset += value.length;
  }
  return transcript;
}

export async function sha256Hex(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function utf8(value: string): Uint8Array<ArrayBuffer> {
  return encoder.encode(value);
}
