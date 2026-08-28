import { beforeEach, describe, expect, it, vi } from "vitest";

import { sha256Hex, utf8 } from "../lib/lengthDelimitedCrypto";

const bridgeMocks = vi.hoisted(() => ({
  create: vi.fn(),
  revoke: vi.fn(),
}));

vi.mock("../lib/desktopBridge", async () => ({
  ...(await vi.importActual<typeof import("../lib/desktopBridge")>(
    "../lib/desktopBridge"
  )),
  fetchDesktopHumanInviteCreate: bridgeMocks.create,
  fetchDesktopHumanInviteRevoke: bridgeMocks.revoke,
}));

import {
  createManagedHumanInvite,
  HumanInviteDispatchError,
  parseManagedHumanInviteCreateResponse,
  revokeManagedHumanInvite,
  type ManagedHumanInviteCreateIntent,
} from "./humanInviteManager";

const authority = {
  server_id: "10000000-0000-4000-8000-000000000001",
  authority_lineage_id: "20000000-0000-4000-8000-000000000002",
  room_id: "general",
  room_uid: "30000000-0000-4000-8000-000000000003",
};

const intent: ManagedHumanInviteCreateIntent = {
  authority,
  displayName: "Guest",
  inviteScope: "room",
  ttlSeconds: 3600,
  maxUses: 1,
};

const inviteToken = `aai1.Y2xhaW1z.${"A".repeat(43)}`;
const joinCode = `aaj1_${"B".repeat(32)}`;

async function exactResponse() {
  return {
    invite_id: (await sha256Hex(utf8(inviteToken))).slice(0, 16),
    invite_token: inviteToken,
    join_code: joinCode,
    meeting_id: authority.room_id,
    agent_id: `guest-${"c".repeat(32)}`,
    display_name: "Guest",
    invite_scope: "room",
    participant_type: "human",
    client_type: "browser",
    provider_kind: "manual",
    permission_mode: "participant",
    max_uses: 1,
    expires_at: "2099-01-01T00:00:00.123456+00:00",
    room_url: "http://127.0.0.1:43123",
    join_url: `https://public.example.test/join?token=${joinCode}`,
  };
}

function response(status: number, body: unknown) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

describe("managed human invite contract", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("accepts one exact create response and retains immutable revoke custody", async () => {
    const custody = await parseManagedHumanInviteCreateResponse(
      await exactResponse(),
      intent
    );

    expect(custody).toEqual({
      authority,
      inviteId: (await sha256Hex(utf8(inviteToken))).slice(0, 16),
      joinUrl: `https://public.example.test/join?token=${joinCode}`,
      responseOrigin: "https://public.example.test",
      expiresAt: {
        exact: "2099-01-01T00:00:00.123456+00:00",
        epochMilliseconds: Date.parse("2099-01-01T00:00:00.123456+00:00"),
      },
    });
    expect(Object.isFrozen(custody)).toBe(true);
    expect(Object.isFrozen(custody.authority)).toBe(true);
    expect(Object.isFrozen(custody.expiresAt)).toBe(true);
  });

  it("rejects response substitution before exposing a join credential", async () => {
    const canonical = await exactResponse();
    const malformed = [
      { ...canonical, invite_id: "0".repeat(16) },
      { ...canonical, meeting_id: "other" },
      { ...canonical, display_name: "Substituted" },
      { ...canonical, max_uses: 5 },
      { ...canonical, ignored: true },
      { ...canonical, expires_at: "2099-01-01T00:00:00Z" },
      { ...canonical, expires_at: "2099-02-30T00:00:00+00:00" },
      {
        ...canonical,
        join_url: `http://public.example.test/join?token=${joinCode}`,
      },
      {
        ...canonical,
        join_url: `https://foo.localhost/join?token=${joinCode}`,
      },
      {
        ...canonical,
        join_url: `https://public.example.test/join?token=${joinCode}&extra=1`,
      },
      {
        ...canonical,
        join_url: `https://public.example.test/join?token=other`,
      },
    ];

    for (const candidate of malformed) {
      await expect(
        parseManagedHumanInviteCreateResponse(candidate, intent)
      ).rejects.toThrow("응답 계약");
    }
  });

  it("sends only the canonical human intent through the manager grant", async () => {
    const payload = await exactResponse();
    bridgeMocks.create.mockImplementation(
      async (_authority, _init, beforeDispatch?: () => void) => {
        beforeDispatch?.();
        return response(200, payload);
      }
    );
    const beforeDispatch = vi.fn();

    const custody = await createManagedHumanInvite(intent, beforeDispatch);

    expect(beforeDispatch).toHaveBeenCalledOnce();
    expect(custody.joinUrl).toContain(joinCode);
    expect(bridgeMocks.create).toHaveBeenCalledWith(
      authority,
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          meeting_id: "general",
          display_name: "Guest",
          invite_scope: "room",
          ttl_seconds: 3600,
          max_uses: 1,
        }),
      }),
      expect.any(Function)
    );
  });

  it("distinguishes native or retired pre-dispatch failure from uncertain dispatch", async () => {
    bridgeMocks.create.mockRejectedValueOnce(new Error("native grant rejected"));
    await expect(createManagedHumanInvite(intent)).rejects.toMatchObject({
      outcome: "proven_not_dispatched",
    });

    bridgeMocks.create.mockImplementationOnce(
      async (_authority, _init, beforeDispatch?: () => void) => {
        beforeDispatch?.();
        throw new Error("transport lost");
      }
    );
    await expect(createManagedHumanInvite(intent)).rejects.toMatchObject({
      outcome: "outcome_unknown",
    });

    bridgeMocks.create.mockImplementationOnce(
      async (_authority, _init, beforeDispatch?: () => void) => {
        beforeDispatch?.();
        return response(200, { malformed: true });
      }
    );
    await expect(createManagedHumanInvite(intent)).rejects.toMatchObject({
      outcome: "outcome_unknown",
    });

    bridgeMocks.create.mockImplementationOnce(
      async (_authority, _init, beforeDispatch?: () => void) => {
        beforeDispatch?.();
        return response(200, await exactResponse());
      }
    );
    const retired = Symbol("retired");
    const caught = await createManagedHumanInvite(intent, () => {
      throw retired;
    }).catch((error: unknown) => error);
    expect(caught).toBeInstanceOf(HumanInviteDispatchError);
    expect(caught).toMatchObject({
      outcome: "proven_not_dispatched",
    });
  });

  it("accepts only exact terminal revoke responses", async () => {
    const custody = await parseManagedHumanInviteCreateResponse(
      await exactResponse(),
      intent
    );
    bridgeMocks.revoke
      .mockImplementationOnce(
        async (_authority, _init, beforeDispatch?: () => void) => {
          beforeDispatch?.();
          return response(200, {
            status: "revoked",
            invite_id: custody.inviteId,
          });
        }
      )
      .mockImplementationOnce(
        async (_authority, _init, beforeDispatch?: () => void) => {
          beforeDispatch?.();
          return response(404, {
            error: { code: "invite_not_found", message: "Invite was not found." },
          });
        }
      )
      .mockImplementationOnce(
        async (_authority, _init, beforeDispatch?: () => void) => {
          beforeDispatch?.();
          return response(404, {
            error: { code: "room_not_found", message: "Room was not found." },
          });
        }
      );

    await expect(revokeManagedHumanInvite(custody)).resolves.toBe("revoked");
    await expect(revokeManagedHumanInvite(custody)).resolves.toBe(
      "invite_not_found"
    );
    await expect(revokeManagedHumanInvite(custody)).rejects.toMatchObject({
      outcome: "outcome_unknown",
    });
  });
});
