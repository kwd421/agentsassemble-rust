import "../../test/nativeDialog";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useContext, useEffect } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { CentralOwnerWorkspaceContext } from "../../lib/centralOwnerWorkspaceContext";
import type { CentralOwnerSessionStatus } from "../../types/generated/CentralOwnerSessionStatus";
import { type CentralOwnerWorkspace } from "../../lib/centralOwnerWorkspace";
import CentralOwnerWorkspaceBoundary from "./CentralOwnerWorkspaceBoundary";

afterEach(() => { cleanup(); vi.useRealTimers(); sessionStorage.clear(); });

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
