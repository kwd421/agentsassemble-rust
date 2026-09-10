import { afterEach, expect, it, vi } from "vitest";
import { createFriendAttendeeInvite, createCompanionAttendeeInvite, attendeePacketText } from "./attendeeInvite";
import { requestDesktopAttendeeInviteCreateTicket } from "../lib/desktopBridge";
vi.mock("../lib/desktopBridge", () => ({ requestDesktopAttendeeInviteCreateTicket: vi.fn() }));
const authority = { server_id: "server", authority_lineage_id: "lineage", room_id: "general", room_uid: "room-uid" };
const request = { request_id: "request", friend_id: "friend" };
const packet = { request_id: "request", room_id: "general", room_uid: "room-uid", invite_id: "invite", expires_at: "2099-01-01T00:00:00Z",
  display_name: "Saved AI", provider: "codex", attend_command: "assemble room attend --provider codex", join_url: "https://room.example.test/join?token=private-invite" };
afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks(); });

it("uses one distinct native ticket and rejects changed room or injected shell arguments", async () => {
  vi.mocked(requestDesktopAttendeeInviteCreateTicket).mockResolvedValue({ ticket: "native-ticket", ttl_seconds: 30, http_base_url: "http://127.0.0.1:8765" });
  const fetch = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify(packet), { status: 200 }));
  vi.stubGlobal("fetch", fetch);
  const assertCurrent = vi.fn();
  const created = await createFriendAttendeeInvite(authority, request, assertCurrent);
  expect(assertCurrent).toHaveBeenCalledOnce();
  expect(fetch).toHaveBeenCalledWith("http://127.0.0.1:8765/api/room-attendee/friend-invite", expect.objectContaining({
    redirect: "error", headers: { Authorization: "Bearer native-ticket", "Content-Type": "application/json" }, body: JSON.stringify(request),
  }));
  expect(attendeePacketText(created.result).split("\n")[0]).toBe(packet.attend_command);
  for (const change of [{ room_uid: "replaced" }, { attend_command: "assemble room attend --provider codex; injected" }, { join_url: `${packet.join_url}&redirect=elsewhere` }]) {
    fetch.mockResolvedValueOnce(new Response(JSON.stringify({ ...packet, ...change }), { status: 200 }));
    await expect(createFriendAttendeeInvite(authority, request, assertCurrent)).rejects.toThrow("현재 방과 일치하지");
  }
});

it("uses the room bearer and paired device for a companion and requires the current public origin", async () => {
  vi.stubGlobal("window", { location: { origin: "https://room.example.test" } });
  const fetch = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify(packet), { status: 200 }));
  vi.stubGlobal("fetch", fetch);
  const session = { sessionToken: "human-session", roomUid: "room-uid", meetingId: "general" };
  const body = { request_id: "request", provider: "codex", display_name: "Companion" };
  await createCompanionAttendeeInvite(session, body);
  expect(requestDesktopAttendeeInviteCreateTicket).not.toHaveBeenCalled();
  expect(fetch).toHaveBeenCalledWith("/api/room-attendee/companion-invite", expect.objectContaining({ headers: { Authorization: "Bearer human-session", "Content-Type": "application/json" } }));
  fetch.mockResolvedValueOnce(new Response(JSON.stringify(packet), { status: 200 }));
  await createCompanionAttendeeInvite({ ...session, sessionToken: "paired-session", deviceToken: "paired-device" }, body);
  expect(fetch).toHaveBeenLastCalledWith("/api/room-attendee/companion-invite", expect.objectContaining({
    headers: { Authorization: "Bearer paired-session", "X-Device-Token": "paired-device", "Content-Type": "application/json" },
  }));
  fetch.mockResolvedValueOnce(new Response(JSON.stringify({ ...packet, join_url: "https://changed.example.test/join?token=private" }), { status: 200 }));
  await expect(createCompanionAttendeeInvite(session, body)).rejects.toThrow("공개 주소가 변경");
});
