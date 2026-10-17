import { describe, expect, it, vi } from "vitest";
import {
  applyConfirmedMembershipLost,
  canonicalRoomAuthFor,
  lostMemberRoomIds,
  memberAdmissionServerFor,
  remoteRoomSessionReady,
  roomPreferenceAuthorityFor,
  serverProductSurfaceFor,
} from "./remoteRoomAuthority";
import type { RoomDockItem } from "../lib/roomDockModel";

const localRoom = { roomOrigin: "local", meetingId: "own-1" } as RoomDockItem;
const memberRoom = {
  roomOrigin: "remote_server", serverId: "friend", meetingId: "hihi-room",
} as RoomDockItem;

describe("remoteRoomAuthority", () => {
  it("routes only anchored member rooms through admission", () => {
    const hasAnchor = (serverId: string) => serverId === "friend";
    expect(memberAdmissionServerFor(memberRoom, "owner-server", hasAnchor)).toBe("friend");
    expect(memberAdmissionServerFor(memberRoom, "friend", hasAnchor)).toBeNull();
    expect(memberAdmissionServerFor(memberRoom, undefined, () => false)).toBeNull();
    expect(memberAdmissionServerFor(localRoom, undefined, hasAnchor)).toBeNull();
  });

  it("never reports a session-less remote room as ready", () => {
    const session = { guestLocked: true, sessionToken: "token", session: {} as never, expired: false };
    expect(remoteRoomSessionReady(memberRoom, session)).toBe(true);
    expect(remoteRoomSessionReady(memberRoom, { ...session, sessionToken: "" })).toBe(false);
    expect(remoteRoomSessionReady(memberRoom, { ...session, session: null })).toBe(false);
    expect(remoteRoomSessionReady(memberRoom, { ...session, expired: true })).toBe(false);
    expect(remoteRoomSessionReady(localRoom, { ...session, sessionToken: "" })).toBe(true);
  });

  it("never issues host auth for a remote room", () => {
    expect(canonicalRoomAuthFor(
      memberRoom, { operational: true, selectError: false, guestLocked: false }, "", "device",
    )).toBeUndefined();
    expect(canonicalRoomAuthFor(
      memberRoom, { operational: false, selectError: false, guestLocked: true }, "token", "device",
    )).toEqual({ kind: "session", sessionToken: "token", deviceToken: "device" });
    expect(canonicalRoomAuthFor(
      localRoom, { operational: false, selectError: false, guestLocked: false }, "", "device",
    )).toEqual({ kind: "host", meetingId: "own-1" });
  });

  it("leaves a session-less remote room without surface or preferences", () => {
    const surface = { websocket_streams: ["room_events"] } as never;
    expect(serverProductSurfaceFor(false, null, null, true, surface)).toBeUndefined();
    expect(roomPreferenceAuthorityFor(false, false, "", "device")).toEqual({ kind: "remote-unavailable" });
    expect(roomPreferenceAuthorityFor(true, false, "", "device")).toEqual({ kind: "local", deviceToken: "device" });
  });

  it("drops only the lost host's rooms and falls back past them", () => {
    const rooms = [
      { ...localRoom, id: "own" },
      { ...memberRoom, id: "member-a" },
      { ...memberRoom, id: "member-b", serverId: "other" },
    ] as RoomDockItem[];
    expect(lostMemberRoomIds(rooms, "friend")).toEqual(["member-a"]);
    const actions = {
      closeHost: vi.fn(), unbindHostSessions: vi.fn(), clearSession: vi.fn(),
      removeRoom: vi.fn(), setSelectError: vi.fn(), activateRoom: vi.fn(),
      resetChannel: vi.fn(), closeAdmin: vi.fn(), clearRetainedRoom: vi.fn(),
    };
    applyConfirmedMembershipLost("friend",
      {
        guestSession: { serverSurface: { server_id: "friend" } } as never, guestExpired: false,
        latestRooms: rooms, activeRoomId: "member-a",
        memberSelectError: { roomId: "member-a", message: "방을 열지 못했어요. 다시 시도해 주세요." },
      }, actions);
    expect(actions.closeHost).toHaveBeenCalledWith("friend");
    expect(actions.unbindHostSessions).toHaveBeenCalledWith("friend");
    expect(actions.clearSession).toHaveBeenCalled();
    expect(actions.removeRoom).toHaveBeenCalledWith("member-a");
    expect(actions.setSelectError).toHaveBeenCalledWith(null);
    expect(actions.activateRoom).toHaveBeenCalledWith("own");
    expect(actions.clearRetainedRoom).toHaveBeenCalled();
  });
});
