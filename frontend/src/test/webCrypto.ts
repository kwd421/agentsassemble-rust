export const TEST_RANDOM_UUID = "123e4567-e89b-42d3-a456-426614174000";

export class TestTextEncoder {
  encode(value: string): Uint8Array {
    return Uint8Array.from(value, (character) => character.charCodeAt(0));
  }
}

async function testDigest(_algorithm: string, source: BufferSource): Promise<ArrayBuffer> {
  const input =
    source instanceof ArrayBuffer
      ? new Uint8Array(source)
      : new Uint8Array(source.buffer, source.byteOffset, source.byteLength);
  const output = new Uint8Array(32);
  input.forEach((byte, index) => {
    output[index % output.length] = (output[index % output.length] + byte + index) & 0xff;
  });
  return output.buffer;
}

export const TEST_WEB_CRYPTO = {
  randomUUID: () => TEST_RANDOM_UUID,
  subtle: { digest: testDigest },
};
