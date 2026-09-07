import { afterEach, expect, it, vi } from "vitest";
import { issueGuestRecoveryCode, redeemGuestRecoveryCode } from "./identity";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";

afterEach(() => vi.unstubAllGlobals());

it("requires the exact human/device pair and refuses an unconfirmed code issuance", async () => {
  const fetcher = vi.fn().mockResolvedValue(new Response(JSON.stringify({
    status: "pending", server_id: "server", room_id: "general", recovery_code: "code", recovery_url: "url",
  })));
  vi.stubGlobal("fetch", fetcher);
  await expect(issueGuestRecoveryCode({ sessionToken: "human" })).rejects.toThrow("기기 정보");
  expect(fetcher).not.toHaveBeenCalled();
  await expect(issueGuestRecoveryCode({ sessionToken: "human", deviceToken: "device" })).rejects.toThrow("발급 결과");
  expect(fetcher).toHaveBeenCalledWith("/api/identity/recovery-code", expect.objectContaining({
    method: "POST", redirect: "error", cache: "no-store",
    headers: { "Content-Type": "application/json", Authorization: "Bearer human", "X-Device-Token": "device" },
  }));
});

it("retains the opaque recovery code and verifies the recovered room/client before accepting", async () => {
  const response = {
    status: "recovered", session_token: "session", recovery_code: "replacement",
    server_id: "11111111-1111-4111-8111-111111111111",
    authority_lineage_id: "22222222-2222-4222-8222-222222222222",
    server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
    agent_id: "human", display_name: "Human", meeting_id: "general", room_uid: "room-uid",
    invite_scope: "read_only", participant_type: "human", client_type: "browser",
    provider_kind: "manual", connection_kind: "native_remote_room_client", client_id: "client",
    joined_at: "2026-09-08T00:00:00Z", expires_at: "2026-09-08T01:00:00Z",
    room_label: "General", room_topic: "", room_created_at: "2026-09-07T00:00:00Z",
  };
  const fetcher = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify(response)))
    .mockResolvedValueOnce(new Response(JSON.stringify({ ...response, client_id: "other" })));
  vi.stubGlobal("fetch", fetcher);
  const input = { recoveryCode: "aagr1.Mixed_Case", roomId: "general", deviceToken: "device", clientId: "client" };
  expect((await redeemGuestRecoveryCode(input)).invite_scope).toBe("read_only");
  expect(fetcher).toHaveBeenCalledWith("/api/identity/recovery-code/redeem", expect.objectContaining({
    redirect: "error", cache: "no-store", body: JSON.stringify({
      recovery_code: input.recoveryCode, room_id: "general", device_token: "device", client_id: "client",
    }),
  }));
  await expect(redeemGuestRecoveryCode(input)).rejects.toThrow("클라이언트");
});
