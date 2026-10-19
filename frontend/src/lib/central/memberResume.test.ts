import { describe, expect, it } from "vitest";
import {
  MEMBER_RESUME_PROTOCOL,
  MEMBER_RESUME_PURPOSE,
  browserCredentialFingerprint,
  memberResumeTranscript,
  parseMemberResumeAnswer,
  parseMemberResumeNonce,
  signMemberResume,
  type MemberResumeTuple,
} from "./memberResume";
import { decodeCanonicalBase64Url } from "../base64Url";

const tuple: MemberResumeTuple = {
  server_id: "server",
  registration_epoch: "epoch",
  origin: "https://host.test",
  generation: 3,
  host_key_fingerprint: "fingerprint",
  browser_credential_fingerprint: "Y3JlZGVudGlhbA",
  client_public_key: "cHVibGljLWtleQ",
  channel_id: "channel",
  nonce: "bm9uY2U",
};

describe("the member resume transcript", () => {
  it("is the client-built tuple and never a host-supplied blob", () => {
    const transcript = new TextDecoder().decode(memberResumeTranscript(tuple));
    expect(transcript).toBe(JSON.stringify([
      "AA-MEMBER-RESUME-1",
      "server",
      "epoch",
      "https://host.test",
      3,
      "fingerprint",
      "Y3JlZGVudGlhbA",
      "cHVibGljLWtleQ",
      "channel",
      "resume",
      "bm9uY2U",
    ]));
    expect(MEMBER_RESUME_PROTOCOL).toBe("AA-MEMBER-RESUME-1");
    expect(MEMBER_RESUME_PURPOSE).toBe("resume");
  });

  it("signs the transcript the device can verify with its own public key", async () => {
    const pair = await crypto.subtle.generateKey(
      { name: "ECDSA", namedCurve: "P-256" },
      false,
      ["sign", "verify"],
    );
    const transcript = memberResumeTranscript(tuple);
    const signature = await signMemberResume(pair.privateKey, transcript);
    const raw = decodeCanonicalBase64Url(signature);
    expect(raw?.length).toBe(64);
    expect(
      await crypto.subtle.verify(
        { name: "ECDSA", hash: "SHA-256" },
        pair.publicKey,
        raw as unknown as BufferSource,
        transcript,
      ),
    ).toBe(true);
    // Another tuple never verifies with the same signature.
    expect(
      await crypto.subtle.verify(
        { name: "ECDSA", hash: "SHA-256" },
        pair.publicKey,
        raw as unknown as BufferSource,
        memberResumeTranscript({ ...tuple, origin: "https://other.test" }),
      ),
    ).toBe(false);
  });

  it("fingerprints the browser credential the host compares", async () => {
    const fingerprint = await browserCredentialFingerprint("aad1_fixture");
    const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode("aad1_fixture"));
    expect(fingerprint).toBe(
      btoa(String.fromCharCode(...new Uint8Array(digest)))
        .replace(/\+/g, "-")
        .replace(/\//g, "_")
        .replace(/=+$/g, ""),
    );
  });
});

describe("the member resume answers", () => {
  it("accepts a fresh nonce and refuses anything else", () => {
    const expires = Math.floor(Date.now() / 1000) + 60;
    expect(parseMemberResumeNonce({ nonce: "n", expires_at: expires })).toEqual({
      nonce: "n",
      expires_at: expires,
    });
    for (const value of [
      {},
      { nonce: "n" },
      { nonce: "n", expires_at: expires, extra: 1 },
      { nonce: "", expires_at: expires },
      { nonce: "n", expires_at: 1 },
      { nonce: "n", expires_at: "soon" },
    ]) {
      expect(() => parseMemberResumeNonce(value)).toThrow();
    }
  });

  it("accepts the joined rooms and refuses anything else", () => {
    const expires = Math.floor(Date.now() / 1000) + 60;
    const answer = parseMemberResumeAnswer({
      rooms: [{ room_id: "general", name: "일반" }],
      challenge_id: "challenge",
      challenge_expires_at: expires,
    });
    expect(answer.rooms).toEqual([{ room_id: "general", name: "일반" }]);
    expect(answer.challenge_id).toBe("challenge");
    for (const value of [
      {},
      { rooms: [], challenge_id: "c" },
      { rooms: [{ room_id: "general" }], challenge_id: "c", challenge_expires_at: expires },
      { rooms: [{ room_id: "", name: "x" }], challenge_id: "c", challenge_expires_at: expires },
      { rooms: [], challenge_id: "", challenge_expires_at: expires },
      { rooms: [], challenge_id: "c", challenge_expires_at: expires, extra: true },
    ]) {
      expect(() => parseMemberResumeAnswer(value)).toThrow();
    }
  });
});
