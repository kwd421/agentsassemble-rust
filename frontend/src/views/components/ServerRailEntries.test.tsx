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
  const button = screen.getByRole("button", { name: "Other Mac · 연결 끊김 · 로그인 서버 확인 불가" });
  expect(screen.queryByRole("button", { name: /This Mac/ })).toBeNull();
  fireEvent.click(button);
  expect(open).toHaveBeenCalledWith(remote);
  rerender(<ServerRailEntries directory={directory} localServerIds={["local"]} connectingId="remote" onOpen={open} />);
  expect(screen.getByRole("button", { name: "Other Mac · 연결 중" })).toBeTruthy();
});

it("keeps server IDs out of unnamed rail labels and tooltips", () => {
  const server = { server_id: "12345678-1234-4234-8234-123456789abc", alias: "", host_os: "macos" as const, relation: "owner" as const };
  render(<ServerRailEntries directory={{ status: "central-unconfirmed", person: null, live: null, servers: [server] }}
    localServerIds={[]} connectingId="" onOpen={vi.fn()} />);
  const button = screen.getByRole("button", { name: /이름 없는 서버/ });
  expect(button.title).not.toContain(server.server_id);
  expect(document.body.textContent).not.toContain(server.server_id);
});

it("uses admitted workspace custody ahead of a stale central list", () => {
  const directory: CentralDirectoryState = { status: "central-unconfirmed", person: null, live: null,
    servers: [{ server_id: "host", alias: "Host", relation: "owner" }] };
  const { rerender } = render(<ServerRailEntries directory={directory} localServerIds={[]}
    connectedServerId="host" connectingId="" onOpen={vi.fn()} />);
  expect(screen.getByRole("button", { name: "Host · 연결됨" })).toBeTruthy();
  rerender(<ServerRailEntries directory={directory} localServerIds={[]} connectingId="" onOpen={vi.fn()} />);
  expect(screen.getByRole("button", { name: "Host · 연결 끊김 · 로그인 서버 확인 불가" })).toBeTruthy();
});

it("offers the same hide action from the member menu without leaving a room",()=>{
  const member={server_id:"friend",registration_epoch:"epoch",alias:"친구의 컴퓨터",relation:"member" as const};
  const hide=vi.fn().mockResolvedValue(undefined);
  render(<ServerRailEntries directory={{status:"connected",person:null,live:null,servers:[member]}} localServerIds={[]} connectingId="" onOpen={vi.fn()} onHide={hide}/>);
  fireEvent.click(screen.getByRole("button",{name:"친구의 컴퓨터 메뉴"}));
  fireEvent.click(screen.getByRole("menuitem",{name:"목록에서 숨기기"}));
  expect(hide).toHaveBeenCalledWith(member);
});
