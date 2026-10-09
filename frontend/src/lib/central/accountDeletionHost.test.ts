import { beforeEach, expect, it, vi } from "vitest";
import { deletionOwnHost, stopDeletionOwnHost } from "./accountDeletionHost";
import { loadCentralSession, signedRequest } from "./identity";
import { isDesktopWebview, requestDesktopAccountDeletionTicket } from "../desktopBridge";
vi.mock("./identity", () => ({ loadCentralSession: vi.fn(), signedRequest: vi.fn() }));
vi.mock("../desktopBridge", () => ({ isDesktopWebview: vi.fn(), requestDesktopAccountDeletionTicket: vi.fn() }));
const session = { token: "current", device_id: "device", expires_at: 2 ** 31, person: { person_id: "person", display_name: "Guest", identity_kind: "guest" as const } };
const owner = { server_id: "server", relation: "owner", registration_epoch: "epoch", host_key_fingerprint: "fingerprint", host_public_key_jwk: { x: "key" }, endpoint: { account_deletion_protocol: "v1" } };
beforeEach(() => { vi.clearAllMocks(); vi.mocked(loadCentralSession).mockReturnValue(session); vi.mocked(isDesktopWebview).mockReturnValue(false); });
it("requires the current signed endpoint capability on every own server before exposure", async () => {
  vi.mocked(signedRequest).mockResolvedValue({ person: session.person, servers: [{ ...owner, endpoint: null }] });
  await expect(deletionOwnHost(session, new AbortController().signal)).rejects.toThrow("업데이트");
  expect(requestDesktopAccountDeletionTicket).not.toHaveBeenCalled();
  vi.mocked(signedRequest).mockResolvedValue({ person: session.person, servers: [owner] });
  expect(await deletionOwnHost(session, new AbortController().signal)).toEqual({ host: null, hasOwnServers: true });
});
it("binds same-computer stop to native installation key and exact epoch, with no fresh proof or receipt", async () => {
  vi.mocked(isDesktopWebview).mockReturnValue(true);
  vi.mocked(signedRequest).mockResolvedValue({ person: session.person, servers: [owner] });
  const ticket = { ticket: "one-use", ttl_seconds: 30, http_base_url: "http://127.0.0.1:45111", server_id: "server", host_key_fingerprint: "fingerprint", host_public_key_x: "key" };
  vi.mocked(requestDesktopAccountDeletionTicket).mockResolvedValue(ticket);
  const own = (await deletionOwnHost(session, new AbortController().signal)).host!;
  const fetch = vi.fn<typeof globalThis.fetch>(async () => Response.json({ status: "account_host_stopped" }));
  vi.stubGlobal("fetch", fetch);
  try {
    await stopDeletionOwnHost(session, own, new AbortController().signal);
    expect(JSON.parse(fetch.mock.calls[0][1]!.body as string)).toEqual({ server_id: "server", registration_epoch: "epoch", account_deletion: true, expected_owner_person_id: "person" });
    vi.mocked(requestDesktopAccountDeletionTicket).mockResolvedValue({ ...ticket, host_public_key_x: "replacement" });
    await expect(stopDeletionOwnHost(session, own, new AbortController().signal)).rejects.toThrow("등록");
    expect(fetch).toHaveBeenCalledOnce();
  } finally { vi.unstubAllGlobals(); }
});
