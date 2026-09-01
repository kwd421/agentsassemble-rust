import {
  lengthDelimitedTranscript,
  sha256Hex,
} from "./lengthDelimitedCrypto";

const CONNECTION_NONCE_CONTEXT = "agentsassemble.ws-connection-nonce.v1";
const HEX_32_BYTES = /^[0-9a-f]{64}$/;

export function isHex32Bytes(value: unknown): value is string {
  return typeof value === "string" && HEX_32_BYTES.test(value);
}

export async function deriveConnectionNonce(ticket: string): Promise<string> {
  return sha256Hex(lengthDelimitedTranscript(CONNECTION_NONCE_CONTEXT, [ticket]));
}
