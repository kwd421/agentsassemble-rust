import "../../test/nativeDialog";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useContext, useEffect } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { CentralOwnerWorkspaceContext } from "../../lib/centralOwnerWorkspaceContext";
import type { CentralOwnerSessionStatus } from "../../types/generated/CentralOwnerSessionStatus";
import { loadCentralOwnerWorkspace, persistCentralOwnerWorkspace, type CentralOwnerWorkspace } from "../../lib/centralOwnerWorkspace";
import CentralOwnerWorkspaceBoundary from "./CentralOwnerWorkspaceBoundary";

afterEach(() => { cleanup(); vi.useRealTimers(); sessionStorage.clear(); });

it("keeps the workspace and draft mounted across five minutes of renewal, then blocks revoked access", () => {
  vi.useFakeTimers();
  const now = Math.floor(Date.now() / 1000);
  const session: CentralOwnerWorkspace = { sessionToken: `aaos1.${"A".repeat(43)}`, serverId: "owner-server", generation: 1,
    expiresAt: now + 86400, leaseExpiresAt: now + 60, hostPublicKeyX: "B".repeat(43), hostKeyFingerprint: "C".repeat(43) };
  persistCentralOwnerWorkspace(session);
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
      publish({ state: "active", expires_at: now + elapsed + 60 });
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("textbox")).toBe(draft);
  }
  expect((draft as HTMLInputElement).value).toBe("작성 중인 내용");
  expect(mounted).toHaveBeenCalledOnce();
  expect(unmounted).not.toHaveBeenCalled();
  act(() => publish({ state: "ended", reason: "revoked" }));
  expect(screen.getByRole("alert").textContent).toContain("로그아웃");
  expect(draft.closest("[inert]")).not.toBeNull();
  expect((draft as HTMLInputElement).value).toBe("작성 중인 내용");
  expect(loadCentralOwnerWorkspace()).toBeNull();
  act(() => publish({ state: "active", expires_at: now + 500 }));
  expect(screen.getByRole("alert")).toBeTruthy();
});

it("blocks an unconfirmed lease after its existing deadline without granting an offline extension", () => {
  vi.useFakeTimers();
  const now = Math.floor(Date.now() / 1000);
  const session: CentralOwnerWorkspace = { sessionToken: `aaos1.${"D".repeat(43)}`, serverId: "server", generation: 1,
    expiresAt: now + 86400, leaseExpiresAt: now + 60, hostPublicKeyX: "B".repeat(43), hostKeyFingerprint: "C".repeat(43) };
  let publish!: (status: CentralOwnerSessionStatus) => void;
  function Workspace() { publish = useContext(CentralOwnerWorkspaceContext)!.onStatus; return <input aria-label="draft" />; }
  render(<CentralOwnerWorkspaceBoundary session={session}><Workspace /></CentralOwnerWorkspaceBoundary>);
  act(() => publish({ state: "retrying", expires_at: now + 60 }));
  expect(screen.queryByRole("alert")).toBeNull();
  act(() => vi.advanceTimersByTime(60_000));
  expect(screen.getByRole("alert").textContent).toContain("확인하지 못했어요");
});
