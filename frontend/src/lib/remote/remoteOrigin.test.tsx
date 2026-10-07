// @vitest-environment-options {"url":"https://central.test/"}
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useRoomDirectory } from "../../app/useRoomDirectory";
import { useRoomAdmission } from "../../app/useRoomAdmission";
import { useCompanionInvites } from "../../app/useCompanionInvites";
import { createCompanionAttendeeInvite } from "../../api/attendeeInvite";
import { TEST_SERVER_PRODUCT_SURFACE } from "../../test/serverProductSurface";
import { currentRoomDirectoryAuthority } from "../roomDirectoryContract";
import { roomFromGuestSession } from "../roomDockModel";
import type { RoomGuestSession } from "../roomGuestSession";
import { closeRemoteWorkspace, installRemoteWorkspace } from "./remoteWorkspace";
import type { RemoteTransport } from "./remoteTransport";
const copy = vi.hoisted(() => vi.fn());
vi.mock("../copyInviteText", () => ({ copyText: copy }));
vi.mock("../roomDirectorySubscription", () => ({ openNativeDirectoryStream: vi.fn(), subscribeRoomDirectory: () => ({ close: vi.fn(), retry: vi.fn() }) }));
afterEach(() => { closeRemoteWorkspace(); vi.restoreAllMocks(); });
function workspace(server: string) {
  const origin = `https://${server}.test`;
  const callbacks = new Set<() => void>();
  const remote = { hello: { origin }, active: true, fetch: vi.fn(), onClose: (fn: () => void) => { callbacks.add(fn); return () => callbacks.delete(fn); }, close: () => { remote.active = false; for (const fn of callbacks) fn(); } };
  const session = { sessionToken: `token-${server}`, meetingId: "general", roomUid: "room-uid", displayName: "Owner", expiresAt: "2099-01-01T00:00:00Z",
    serverSurface: { server_id: server, authority_lineage_id: `lineage-${server}`, server_product_surface: TEST_SERVER_PRODUCT_SURFACE } } as RoomGuestSession;
  installRemoteWorkspace({ transport: remote as unknown as RemoteTransport, owner: { sessionToken: session.sessionToken } as never, deviceToken: "device", clientId: "client" });
  return { remote, session, origin };
}
it("binds directories and dock entries to host origins across A to B in one trusted shell", async () => {
  for (const server of ["server-a", "server-b"]) {
    const { session, origin } = workspace(server);
    const payload = { ...session.serverSurface, profile_revision: 1, rooms: [{ room_id: "general", room_uid: "room-uid", label: "General", status: "active", archived: false, cleanup_pending: false, deletion_pending: false, origin: "agent_session", last_active_at: "" }] };
    const remoteOwner = { serverId: server, fetchRooms: vi.fn().mockResolvedValue(payload), onStatus: vi.fn(), openStream: vi.fn() };
    const hook = renderHook(() => useRoomDirectory({ initialRooms: [], hostEnabled: false, remoteOwner }));
    await waitFor(() => expect(hook.result.current.syncIssue).toBeNull());
    expect(hook.result.current.rooms[0].serverOrigin).toBe(origin);
    expect(currentRoomDirectoryAuthority()).toEqual({ server_id: server, authority_lineage_id: `lineage-${server}` });
    expect(currentRoomDirectoryAuthority(window.location.origin)).toBeNull();
    hook.unmount(); closeRemoteWorkspace();
  }
});
it("binds member surfaces and docks under the signed host, not the shell", async () => {
  const { session, origin } = workspace("member-host");
  const hook = renderHook(() => useRoomAdmission({ deviceToken: "device", clientId: "client", guestInvite: null,
    guestJoinToken: "", operatorPairingToken: "", initialSession: session,
    onPairingTokenConsumed: vi.fn(), onRoomJoined: vi.fn(), onResetToLobby: vi.fn() }));
  await waitFor(() => expect(hook.result.current.admittedSessionToken).toBe(session.sessionToken));
  expect(currentRoomDirectoryAuthority(origin)?.server_id).toBe("member-host");
  expect(roomFromGuestSession(session).serverOrigin).toBe(origin);
  hook.unmount();
});
it("creates and copies companion invites on the signed host while rejecting an origin substitution", async () => {
  const { remote, session, origin } = workspace("companion-host");
  const body = { request_id: "request", provider: "codex", display_name: "Companion" };
  const packet = { ...body, room_id: "general", room_uid: "room-uid", invite_id: "invite", expires_at: new Date(Date.now() + 60_000).toISOString(), attend_command: "assemble room attend --provider codex", join_url: `${origin}/join?token=fixture` };
  remote.fetch.mockImplementation(async (_path, init) => new Response(JSON.stringify({ ...packet, request_id: JSON.parse(init.body).request_id })));
  await expect(createCompanionAttendeeInvite(session, body)).resolves.toMatchObject({ origin });
  const hook = renderHook(() => useCompanionInvites(session));
  act(() => { hook.result.current.setProvider("codex"); hook.result.current.setDisplayName("Companion"); });
  await act(() => hook.result.current.create());
  expect(hook.result.current.invites[0].copyable).toBe(true);
  copy.mockImplementation(async (_text, prepare) => { (await prepare())(); return true; });
  await act(() => hook.result.current.copy("invite"));
  expect(copy).toHaveBeenCalledOnce();
  remote.fetch.mockResolvedValueOnce(new Response(JSON.stringify({ ...packet, join_url: "https://wrong.test/join?token=fixture" })));
  await expect(createCompanionAttendeeInvite(session, body)).rejects.toThrow("공개 주소가 변경");
  hook.unmount();
});
