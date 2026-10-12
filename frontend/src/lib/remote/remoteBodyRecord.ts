// HTTP body plaintext inside the existing ordered AEAD record.
const HEADER = 18;
const CHUNK = 64 * 1024;
export const BODY_BINARY = "x-agentsassemble-body-binary";
export const BODY_BINARY_CONFIRMED = "x-agentsassemble-body-binary-confirmed";

export function encodeBodyRecord(id: number, offset: number, bytes: Uint8Array, end: boolean): Uint8Array<ArrayBuffer> {
  const result = new Uint8Array(HEADER + bytes.length);
  const view = new DataView(result.buffer);
  result[0] = 1; result[1] = end ? 1 : 0;
  view.setUint32(2, id); view.setBigUint64(6, BigInt(offset)); view.setUint32(14, bytes.length);
  result.set(bytes, HEADER); return result;
}

export function decodeBodyRecord(bytes: Uint8Array<ArrayBuffer>) {
  if (bytes.length < HEADER || bytes[0] !== 2 || bytes[1] > 1) throw new Error("Invalid encrypted body record");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const id = view.getUint32(2), offset = Number(view.getBigUint64(6)), length = view.getUint32(14);
  const end = bytes[1] === 1;
  if (!id || !Number.isSafeInteger(offset + length) || length > CHUNK || bytes.length !== HEADER + length ||
      (end ? length !== 0 : length === 0)) throw new Error("Invalid encrypted body record");
  return { id, offset, bytes: bytes.slice(HEADER), end };
}
