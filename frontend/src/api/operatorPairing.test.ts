import { afterEach, expect, it, vi } from "vitest";
import { createOperatorPairing, revokeOperatorPairing } from "./operatorPairing";

const http = vi.hoisted(() => ({ post: vi.fn() }));
vi.mock("./http", () => ({ postJsonServerOperator: http.post }));
const authority = {
  server_id: "10000000-0000-4000-8000-000000000001",
  authority_lineage_id: "20000000-0000-4000-8000-000000000001",
  room_id: "general", room_uid: "30000000-0000-4000-8000-000000000001",
};
const response = {
  pairing_id: "40000000-0000-4000-8000-000000000001",
  pairing_url: `https://pair.example.test/pair?token=aap1.${"a".repeat(43)}`,
  expires_at: "2026-09-08T00:02:00Z",
};
afterEach(() => http.post.mockReset());

it("retains exact room authority and accepts only the matching revoke acknowledgement", async () => {
  http.post.mockResolvedValueOnce(response);
  const custody = await createOperatorPairing(authority, vi.fn());
  expect(custody.authority).toEqual(authority);
  http.post.mockResolvedValueOnce({ status: "revoked", pairing_id: "another-pairing" });
  await expect(revokeOperatorPairing(custody, vi.fn())).rejects.toThrow("연결 해제 결과");
  expect(http.post.mock.calls[1].slice(0, 2)).toEqual([
    "/api/operator-pairing/revoke", { authority, pairing_id: response.pairing_id },
  ]);
});

it.each([
  "http://pair.example.test/pair?token=aap1." + "a".repeat(43),
  "https://pair.example.test/join?token=aap1." + "a".repeat(43),
  response.pairing_url + "&redirect=https://another.example.test",
])("rejects a non-contract pairing URL before it becomes copyable: %s", async (pairing_url) => {
  http.post.mockResolvedValueOnce({ ...response, pairing_url });
  await expect(createOperatorPairing(authority, vi.fn())).rejects.toThrow();
});
