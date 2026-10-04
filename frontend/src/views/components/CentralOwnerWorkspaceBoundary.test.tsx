import "../../test/nativeDialog";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useContext, useEffect } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { CentralOwnerWorkspaceContext } from "../../lib/centralOwnerWorkspaceContext";
import type { CentralOwnerSessionStatus } from "../../types/generated/CentralOwnerSessionStatus";
import { type CentralOwnerWorkspace } from "../../lib/centralOwnerWorkspace";
import { createCentralOwnerRoom, enterCentralOwnerRoom, fetchCentralOwnerRooms } from "../../lib/centralOwnerWorkspace";
import { listOwnerDevices, revokeOwnerDevices } from "../../api/ownerDevices";
import { fetchJsonWithIdentity, postJsonWithIdentity } from "../../api/http";
import { fetchSavedFriends } from "../../api/friends";
import { changeRoomLifecycle } from "../../api/roomLifecycle";
import CentralOwnerWorkspaceBoundary from "./CentralOwnerWorkspaceBoundary";

afterEach(() => { cleanup(); vi.useRealTimers(); sessionStorage.clear(); vi.unstubAllGlobals(); });

it("keeps the workspace and draft mounted across five minutes without renewal, then blocks revoked access", () => {
  vi.useFakeTimers();
  const session: CentralOwnerWorkspace = { sessionToken: `aaos1.${"A".repeat(43)}`, serverId: "owner-server", generation: 1,
    sessionId: "30000000-0000-4000-8000-000000000003", hostPublicKeyX: "B".repeat(43), hostKeyFingerprint: "C".repeat(43) };
  const mounted = vi.fn();
  const unmounted = vi.fn();
  let publish!: (status: CentralOwnerSessionStatus) => void;
  function Workspace() {
    publish = useContext(CentralOwnerWorkspaceContext)!.onStatus;
    useEffect(() => { mounted(); return unmounted; }, []);
    return <input aria-label="메시지 작성" defaultValue="" />;
  }
  render(<CentralOwnerWorkspaceBoundary session={session}><Workspace /></CentralOwnerWorkspaceBoundary>);
  const draft = screen.getByRole("textbox");
  fireEvent.change(draft, { target: { value: "작성 중인 내용" } });
  for (let elapsed = 20; elapsed <= 380; elapsed += 20) {
    act(() => {
      vi.advanceTimersByTime(20_000);
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("textbox")).toBe(draft);
  }
  expect((draft as HTMLInputElement).value).toBe("작성 중인 내용");
  expect(mounted).toHaveBeenCalledOnce();
  expect(unmounted).not.toHaveBeenCalled();
  act(() => publish({ state: "ended", reason: "revoked" }));
  expect(screen.getByRole("alert").textContent).toContain("연결을 해제");
  expect(draft.closest("[inert]")).not.toBeNull();
  expect((draft as HTMLInputElement).value).toBe("작성 중인 내용");
  act(() => publish({ state: "active" }));
  expect(screen.getByRole("alert")).toBeTruthy();
});

it("blocks on a host disconnect and never restarts from a late active event", () => {
  const session: CentralOwnerWorkspace = { sessionToken: `aaos1.${"D".repeat(43)}`, serverId: "server", generation: 1,
    sessionId: "30000000-0000-4000-8000-000000000003", hostPublicKeyX: "B".repeat(43), hostKeyFingerprint: "C".repeat(43) };
  let publish!: (status: CentralOwnerSessionStatus) => void;
  function Workspace() { publish = useContext(CentralOwnerWorkspaceContext)!.onStatus; return <input aria-label="draft" />; }
  render(<CentralOwnerWorkspaceBoundary session={session}><Workspace /></CentralOwnerWorkspaceBoundary>);
  act(() => publish({ state: "ended", reason: "disconnected" }));
  expect(screen.getByRole("alert").textContent).toContain("연결이 끊겼어요");
  act(() => publish({ state: "active" }));
  expect(screen.getByRole("alert")).toBeTruthy();
});

const rejectedSession: CentralOwnerWorkspace = { sessionToken: `aaos1.${"E".repeat(43)}`, serverId: "server", generation: 1,
  sessionId: "30000000-0000-4000-8000-000000000003", hostPublicKeyX: "B".repeat(43), hostKeyFingerprint: "C".repeat(43) };
const identity = { centralSession: rejectedSession, deviceToken: "device" };
const rootRequests: Array<[string, () => Promise<unknown>]> = [
  ["directory", () => fetchCentralOwnerRooms(rejectedSession, "device")],
  ["create", () => createCentralOwnerRoom(rejectedSession, "device", "request", "room", "Room")],
  ["enter", () => enterCentralOwnerRoom(rejectedSession, "device", "room", "uid")],
  ["devices", () => listOwnerDevices(identity)],
  ["revoke", () => revokeOwnerDevices(identity, { scope: "all" })],
  ["profile", () => fetchJsonWithIdentity("/api/user-profile", identity)],
  ["profile edit", () => postJsonWithIdentity("/api/user-profile", {}, identity)],
  ["avatar", () => postJsonWithIdentity("/api/attachments", {}, identity)],
  ["friends", () => fetchSavedFriends({ kind: "server_owner", credential: rejectedSession, deviceToken: "device" })],
  ["lifecycle", () => changeRoomLifecycle({ serverId: "server", authorityLineageId: "lineage", requestId: "request", roomId: "room", roomUid: "uid", action: "room.close" }, vi.fn(),
    { kind: "server_owner", credential: rejectedSession, deviceToken: "device" })],
];

it.each(rootRequests)("ends the workspace on rejected root %s requests, preserving drafts", async (_, request) => {
  for (const status of [401, 403]) {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response('{"error":"denied"}', { status })));
    const view = render(<CentralOwnerWorkspaceBoundary session={rejectedSession}><input aria-label="draft" defaultValue="kept" /></CentralOwnerWorkspaceBoundary>);
    const draft = screen.getByRole("textbox") as HTMLInputElement;
    await act(async () => { await expect(request()).rejects.toBeTruthy(); });
    expect(screen.getByRole("alert").textContent).toContain("연결이 끊겼어요");
    expect(draft.closest("[inert]")).not.toBeNull();
    expect(draft.value).toBe("kept");
    view.unmount();
  }
});

it("ignores transient errors and foreign-session rejection, and removes the listener on unmount", async () => {
  const view = render(<CentralOwnerWorkspaceBoundary session={rejectedSession}><input aria-label="draft" /></CentralOwnerWorkspaceBoundary>);
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response('{}', { status: 503 })));
  await act(async () => { await expect(fetchCentralOwnerRooms(rejectedSession, "device")).rejects.toBeTruthy(); });
  expect(screen.queryByRole("alert")).toBeNull();
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response('{}', { status: 401 })));
  await act(async () => { await expect(fetchCentralOwnerRooms({ ...rejectedSession, sessionToken: "another-session" }, "device")).rejects.toBeTruthy(); });
  expect(screen.queryByRole("alert")).toBeNull();
  view.unmount();
  await expect(fetchCentralOwnerRooms(rejectedSession, "device")).rejects.toBeTruthy();
});
