import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import ServerRailEntries from "./ServerRailEntries";
import type { CentralDirectoryState } from "../../app/useCentralDirectory";
afterEach(cleanup);
it("shows saved remote servers as disconnected, excludes this host and reports a connecting attempt", () => {
  const remote = { server_id: "remote", alias: "Other Mac", host_os: "macos" as const, relation: "owner" as const };
  const directory: CentralDirectoryState = { status: "central-unconfirmed", person: null, live: null,
    servers: [{ ...remote, server_id: "local", alias: "This Mac" }, remote] };
  const open = vi.fn().mockResolvedValue(undefined);
  const { rerender } = render(<ServerRailEntries directory={directory} localServerIds={["local"]} connectingId="" onOpen={open} />);
  const button = screen.getByRole("button", { name: "Other Mac · 연결 끊김 · 중앙 확인 불가" });
  expect(screen.queryByRole("button", { name: /This Mac/ })).toBeNull();
  fireEvent.click(button);
  expect(open).toHaveBeenCalledWith(remote);
  rerender(<ServerRailEntries directory={directory} localServerIds={["local"]} connectingId="remote" onOpen={open} />);
  expect(screen.getByRole("button", { name: "Other Mac · 연결 중" })).toBeTruthy();
});
