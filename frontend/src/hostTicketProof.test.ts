import { describe, expect, it } from "vitest";
import {
  signHostTicketRequest,
  verifyHostChallenge,
  verifyHostTicketResponse,
} from "./hostTicketProof";

const HOST_TOKEN = "host-token-0000000000000000000000";
const CHALLENGE = "c".repeat(64);

describe("standalone host ticket proof", () => {
  it("matches the cross-language request and response vectors", async () => {
    await expect(
      signHostTicketRequest(HOST_TOKEN, CHALLENGE, "general")
    ).resolves.toBe("d09a57843280b3ff939f5aac64629f11a93b2ef4f536972bcc962974ef78152c");
    await expect(verifyHostChallenge(HOST_TOKEN, {
      challenge: CHALLENGE,
      host_challenge_proof: "663e4232010a2500a2ee27f392029d1f8bdb8c03ab75e34bfdf39cff63d77144",
    })).resolves.toBe(true);
    await expect(verifyHostTicketResponse(HOST_TOKEN, CHALLENGE, {
      ticket: "a".repeat(64),
      ttl_seconds: 30,
      server_proof_key: "b".repeat(64),
      host_response_proof: "8fe3585f8818361e6eaa5d2a3c91d6da17686881d667e1b2b1a292954dd0d486",
    })).resolves.toBe(true);
  });

  it("rejects forged challenge and ticket grants", async () => {
    await expect(verifyHostChallenge(HOST_TOKEN, {
      challenge: CHALLENGE,
      host_challenge_proof: "0".repeat(64),
    })).resolves.toBe(false);
    await expect(verifyHostTicketResponse(HOST_TOKEN, CHALLENGE, {
      ticket: "a".repeat(64),
      ttl_seconds: 30,
      server_proof_key: "b".repeat(64),
      host_response_proof: "0".repeat(64),
    })).resolves.toBe(false);
  });
});
