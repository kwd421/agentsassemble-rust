import { describe, expect, it } from "vitest";

import type { DesktopCentralRegistrationBinding } from "./desktopBridge";
import {
  type HostRegistrationEnvelope,
  verifyCentralRegistrationEnvelope,
} from "./centralRegistrationProof";

const SERVER_ID = "0198f492-c76a-7000-8000-000000000001";
const OWNER_ID = "per_owner_12345678";

function bytesToBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

async function signedFixture(): Promise<{
  binding: DesktopCentralRegistrationBinding;
  envelope: HostRegistrationEnvelope;
}> {
  const keyPair = (await crypto.subtle.generateKey(
    { name: "Ed25519" },
    true,
    ["sign", "verify"]
  )) as CryptoKeyPair;
  const publicKey = new Uint8Array(await crypto.subtle.exportKey("raw", keyPair.publicKey));
  const x = bytesToBase64Url(publicKey);
  const jwk = {
    crv: "Ed25519" as const,
    ext: true as const,
    key_ops: ["verify"] as ["verify"],
    kty: "OKP" as const,
    x,
  };
  const fingerprint = bytesToBase64Url(
    new Uint8Array(
      await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify(jwk)))
    )
  );
  const issuedAt = 1_788_000_000;
  const nonce = bytesToBase64Url(new Uint8Array(18).fill(7));
  const transcript = `AA-HOST-REGISTER-1\n${SERVER_ID}\n${OWNER_ID}\n${issuedAt}\n${nonce}`;
  const signature = bytesToBase64Url(
    new Uint8Array(
      await crypto.subtle.sign(
        { name: "Ed25519" },
        keyPair.privateKey,
        new TextEncoder().encode(transcript)
      )
    )
  );
  return {
    binding: {
      server_id: SERVER_ID,
      host_public_key_x: x,
      host_key_fingerprint: fingerprint,
    },
    envelope: {
      server_id: SERVER_ID,
      host_public_key_jwk: jwk,
      host_key_fingerprint: fingerprint,
      host_registration_proof: {
        owner_person_id: OWNER_ID,
        issued_at: issuedAt,
        nonce,
        signature,
      },
    },
  };
}

describe("central registration proof authority", () => {
  it("accepts the exact envelope signed by the native-bound host key", async () => {
    const fixture = await signedFixture();
    await expect(
      verifyCentralRegistrationEnvelope(fixture.envelope, OWNER_ID, fixture.binding)
    ).resolves.toEqual(fixture.envelope);
  });

  it("rejects a self-consistent proof signed by a substituted loopback key", async () => {
    const trusted = await signedFixture();
    const substituted = await signedFixture();
    await expect(
      verifyCentralRegistrationEnvelope(substituted.envelope, OWNER_ID, trusted.binding)
    ).rejects.toThrow("native 권위");
  });
});
