const PROOF_CONTEXT = "agentsassemble-server-proof-v1\0";
const HEX_32_BYTES = /^[0-9a-f]{64}$/i;

export function createServerChallenge(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export async function verifyServerProof(
  proofKey: string,
  challenge: string,
  proof: string
): Promise<boolean> {
  if (!HEX_32_BYTES.test(proofKey) || !HEX_32_BYTES.test(challenge) || !HEX_32_BYTES.test(proof)) {
    return false;
  }
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(proofKey),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["verify"]
  );
  const signature = Uint8Array.from(proof.match(/../g) || [], (byte) => Number.parseInt(byte, 16));
  const payload = new TextEncoder().encode(`${PROOF_CONTEXT}${challenge}`);
  return crypto.subtle.verify("HMAC", key, signature, payload);
}
