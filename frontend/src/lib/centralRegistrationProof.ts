import type { DesktopCentralRegistrationBinding } from "./desktopBridge";

const REGISTRATION_CONTEXT = "AA-HOST-REGISTER-1";

type HostPublicJwk = {
  crv: "Ed25519";
  ext: true;
  key_ops: ["verify"];
  kty: "OKP";
  x: string;
};

export type HostRegistrationEnvelope = {
  server_id: string;
  host_public_key_jwk: HostPublicJwk;
  host_key_fingerprint: string;
  host_registration_proof: {
    owner_person_id: string;
    issued_at: number;
    nonce: string;
    signature: string;
  };
};

function exactObject(
  value: unknown,
  keys: readonly string[],
  label: string
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} 형식이 올바르지 않습니다.`);
  }
  const object = value as Record<string, unknown>;
  const actual = Object.keys(object).sort();
  const expected = [...keys].sort();
  if (
    actual.length !== expected.length ||
    actual.some((key, index) => key !== expected[index])
  ) {
    throw new Error(`${label} 계약이 일치하지 않습니다.`);
  }
  return object;
}

function bytesToBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

function decodeBase64Url(
  value: unknown,
  expectedBytes: number,
  label: string
): Uint8Array<ArrayBuffer> {
  if (typeof value !== "string" || !/^[A-Za-z0-9_-]+$/.test(value) || value.length % 4 === 1) {
    throw new Error(`${label} 인코딩이 올바르지 않습니다.`);
  }
  let binary: string;
  try {
    const padded = value.replace(/-/g, "+").replace(/_/g, "/").padEnd(
      value.length + ((4 - (value.length % 4)) % 4),
      "="
    );
    binary = atob(padded);
  } catch {
    throw new Error(`${label} 인코딩이 올바르지 않습니다.`);
  }
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  if (bytes.length !== expectedBytes || bytesToBase64Url(bytes) !== value) {
    throw new Error(`${label} 길이 또는 정규형이 올바르지 않습니다.`);
  }
  return bytes;
}

function exactPublicJwk(value: unknown, expectedX: string): HostPublicJwk {
  const jwk = exactObject(value, ["crv", "ext", "key_ops", "kty", "x"], "호스트 공개키");
  if (
    jwk.crv !== "Ed25519" ||
    jwk.ext !== true ||
    !Array.isArray(jwk.key_ops) ||
    jwk.key_ops.length !== 1 ||
    jwk.key_ops[0] !== "verify" ||
    jwk.kty !== "OKP" ||
    jwk.x !== expectedX
  ) {
    throw new Error("호스트 공개키가 native 권위와 일치하지 않습니다.");
  }
  decodeBase64Url(jwk.x, 32, "호스트 공개키");
  return jwk as unknown as HostPublicJwk;
}

export async function verifyCentralRegistrationEnvelope(
  value: unknown,
  expectedOwnerPersonId: string,
  binding: DesktopCentralRegistrationBinding
): Promise<HostRegistrationEnvelope> {
  const envelope = exactObject(
    value,
    [
      "server_id",
      "host_public_key_jwk",
      "host_key_fingerprint",
      "host_registration_proof",
    ],
    "호스트 등록 증명"
  );
  if (
    envelope.server_id !== binding.server_id ||
    envelope.host_key_fingerprint !== binding.host_key_fingerprint
  ) {
    throw new Error("호스트 등록 증명이 native 권위와 일치하지 않습니다.");
  }
  const jwk = exactPublicJwk(envelope.host_public_key_jwk, binding.host_public_key_x);
  const canonicalJwk = JSON.stringify({
    crv: jwk.crv,
    ext: jwk.ext,
    key_ops: jwk.key_ops,
    kty: jwk.kty,
    x: jwk.x,
  });
  const fingerprint = bytesToBase64Url(
    new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(canonicalJwk)))
  );
  if (fingerprint !== binding.host_key_fingerprint) {
    throw new Error("호스트 공개키 지문이 native 권위와 일치하지 않습니다.");
  }

  const proof = exactObject(
    envelope.host_registration_proof,
    ["owner_person_id", "issued_at", "nonce", "signature"],
    "호스트 등록 서명"
  );
  if (
    proof.owner_person_id !== expectedOwnerPersonId ||
    !Number.isSafeInteger(proof.issued_at) ||
    Number(proof.issued_at) < 1
  ) {
    throw new Error("호스트 등록 서명의 소유자 또는 시간이 올바르지 않습니다.");
  }
  const publicKeyBytes = decodeBase64Url(jwk.x, 32, "호스트 공개키");
  const nonce = decodeBase64Url(proof.nonce, 18, "호스트 등록 nonce");
  const signature = decodeBase64Url(proof.signature, 64, "호스트 등록 서명");
  const transcript = `${REGISTRATION_CONTEXT}\n${binding.server_id}\n${expectedOwnerPersonId}\n${proof.issued_at}\n${bytesToBase64Url(nonce)}`;
  const publicKey = await crypto.subtle.importKey(
    "raw",
    publicKeyBytes,
    { name: "Ed25519" },
    false,
    ["verify"]
  );
  if (
    !(await crypto.subtle.verify(
      { name: "Ed25519" },
      publicKey,
      signature,
      new TextEncoder().encode(transcript)
    ))
  ) {
    throw new Error("호스트 등록 서명이 native 권위로 검증되지 않았습니다.");
  }
  return envelope as unknown as HostRegistrationEnvelope;
}
