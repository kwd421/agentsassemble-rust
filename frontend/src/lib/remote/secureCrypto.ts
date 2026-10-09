import { decodeCanonicalBase64Url, encodeBase64Url } from "../base64Url.ts";

export const SECURE_PROTOCOL = "secure_admission_v1";
export type SecureTarget = {
  server_id: string; registration_epoch: string; origin: string; generation: number;
  host_public_key_jwk: JsonWebKey; host_key_fingerprint: string;
};
export type SecurePurpose = "owner" | "member_admission" | "member_connect" | "account_deletion";
export type ClientHello = {
  protocol: typeof SECURE_PROTOCOL; server_id: string; registration_epoch: string;
  origin: string; generation: number; purpose: SecurePurpose; client_nonce: string; client_public_key: string;
};
export type ServerHello = ClientHello & {
  host_ephemeral_public_key: string; channel_id: string; issued_at: number; signature: string;
};
const encoder = new TextEncoder();
const failure = () => new Error("서버를 확인하지 못했어요. 서버 목록에서 다시 연결해 주세요.");
function bytes(value: string, length: number) {
  const decoded = decodeCanonicalBase64Url(value);
  if (!decoded || decoded.length !== length) throw failure();
  return decoded;
}

export async function beginSecureHandshake(target: SecureTarget, purpose: SecurePurpose) {
  const origin = new URL(target.origin);
  if (origin.protocol !== "https:" || origin.origin !== target.origin || !target.server_id || !target.registration_epoch ||
      !Number.isSafeInteger(target.generation) || target.generation < 1) throw failure();
  const pair = await crypto.subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, false, ["deriveBits"]);
  const hello: ClientHello = {
    protocol: SECURE_PROTOCOL, server_id: target.server_id, registration_epoch: target.registration_epoch,
    origin: target.origin, generation: target.generation, purpose,
    client_nonce: encodeBase64Url(crypto.getRandomValues(new Uint8Array(32))),
    client_public_key: encodeBase64Url(await crypto.subtle.exportKey("raw", pair.publicKey)),
  };
  let finished = false;
  return {
    hello,
    async finish(value: unknown): Promise<{ hello: ServerHello; send: SecureCipher; receive: SecureCipher }> {
      if (finished) throw failure();
      finished = true;
      if (!value || typeof value !== "object" || Array.isArray(value)) throw failure();
      const server = value as ServerHello;
      const expected = [...Object.keys(hello), "host_ephemeral_public_key", "channel_id", "issued_at", "signature"].sort().join();
      if (Object.keys(server).sort().join() !== expected || Object.keys(hello).some(key => server[key as keyof ClientHello] !== hello[key as keyof ClientHello]) ||
          !Number.isSafeInteger(server.issued_at) || Math.abs(server.issued_at - Date.now() / 1000) > 60) throw failure();
      bytes(server.channel_id, 32);
      const hostKey = target.host_public_key_jwk;
      if (hostKey.kty !== "OKP" || hostKey.crv !== "Ed25519" || typeof hostKey.x !== "string") throw failure();
      const hostBytes = bytes(hostKey.x, 32);
      if (encodeBase64Url(await crypto.subtle.digest("SHA-256", encoder.encode(JSON.stringify({ crv: "Ed25519", ext: true, key_ops: ["verify"], kty: "OKP", x: hostKey.x })))) !== target.host_key_fingerprint) throw failure();
      const transcript = encoder.encode(JSON.stringify([
        "AA-SECURE-ADMISSION-1", server.protocol, server.server_id, server.registration_epoch, server.origin, server.generation,
        server.purpose, server.client_nonce, server.client_public_key, server.host_ephemeral_public_key, server.channel_id, server.issued_at,
      ]));
      const host = await crypto.subtle.importKey("raw", hostBytes, "Ed25519", false, ["verify"]);
      if (!await crypto.subtle.verify("Ed25519", host, bytes(server.signature, 64), transcript)) throw failure();
      const peer = await crypto.subtle.importKey("raw", bytes(server.host_ephemeral_public_key, 65), { name: "ECDH", namedCurve: "P-256" }, false, []);
      const secret = await crypto.subtle.deriveBits({ name: "ECDH", public: peer }, pair.privateKey, 256);
      const material = await crypto.subtle.importKey("raw", secret, "HKDF", false, ["deriveKey"]);
      new Uint8Array(secret).fill(0);
      const derive = async (direction: "c2h" | "h2c") => {
        const label = encoder.encode(direction);
        const info = new Uint8Array(transcript.length + label.length);
        info.set(transcript); info.set(label, transcript.length);
        const key = await crypto.subtle.deriveKey({ name: "HKDF", hash: "SHA-256", salt: bytes(hello.client_nonce, 32), info }, material,
          { name: "AES-GCM", length: 256 }, false, direction === "c2h" ? ["encrypt"] : ["decrypt"]);
        return new SecureCipher(key, server.channel_id, direction);
      };
      return { hello: server, send: await derive("c2h"), receive: await derive("h2c") };
    },
  };
}

export class SecureCipher {
  private counter = 0n;
  private key: CryptoKey;
  private channel: string;
  private direction: "c2h" | "h2c";
  constructor(key: CryptoKey, channel: string, direction: "c2h" | "h2c") { this.key = key; this.channel = channel; this.direction = direction; }
  private parameters() {
    if (this.counter >= (1n << 64n) - 1n) throw failure();
    const iv = new Uint8Array(12);
    new DataView(iv.buffer).setBigUint64(4, this.counter);
    return { name: "AES-GCM", iv, additionalData: encoder.encode(JSON.stringify([SECURE_PROTOCOL, this.channel, this.direction, this.counter.toString()])) };
  }
  async seal(plaintext: Uint8Array<ArrayBuffer>): Promise<ArrayBuffer> {
    if (this.direction !== "c2h" || plaintext.length > 96 * 1024) throw failure();
    const ciphertext = await crypto.subtle.encrypt(this.parameters(), this.key, plaintext);
    const record = new Uint8Array(8 + ciphertext.byteLength);
    new DataView(record.buffer).setBigUint64(0, this.counter);
    record.set(new Uint8Array(ciphertext), 8);
    this.counter += 1n;
    return record.buffer;
  }
  async open(record: ArrayBuffer): Promise<Uint8Array<ArrayBuffer>> {
    if (this.direction !== "h2c" || record.byteLength < 24 || record.byteLength > 96 * 1024 + 24 ||
        new DataView(record).getBigUint64(0) !== this.counter) throw failure();
    const plaintext = await crypto.subtle.decrypt(this.parameters(), this.key, record.slice(8));
    this.counter += 1n;
    return new Uint8Array(plaintext);
  }
}
