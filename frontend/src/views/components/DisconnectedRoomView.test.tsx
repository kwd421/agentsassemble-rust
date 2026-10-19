import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createFreshRoom } from "../../lib/roomDockModel";
import { projectRoomConnections } from "../../lib/serverConnectionState";
import DisconnectedRoomView from "./DisconnectedRoomView";
afterEach(cleanup);
it("projects local stream failure, shows its reason and retries; successful stream restores it", async () => {
  const original = createFreshRoom();
  const [offline] = projectRoomConnections([original], "disconnected", true);
  const retry = vi.fn().mockResolvedValue(undefined);
  render(<DisconnectedRoomView room={offline} onRetry={retry} />);
  expect(screen.getByText(/이 기기의 서버 연결이 끊겼어요/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "다시 연결" }));
  await vi.waitFor(() => expect(retry).toHaveBeenCalledOnce());
  expect(projectRoomConnections([original], "connected", true)[0].connectionState).toBe("local");
});
it("explains central-unconfirmed remote rooms without hiding saved room identity", () => {
  const [offline] = projectRoomConnections([{ ...createFreshRoom(), roomOrigin: "remote_server", connectionState: "disconnected" }], null, true);
  render(<DisconnectedRoomView room={offline} onRetry={async () => {}} />);
  expect(screen.getByText(/로그인 서버에 연결하지 못했어요/)).toBeTruthy();
  expect(screen.getByRole("heading", { name: `‘${offline.label}’에 연결할 수 없어요` })).toBeTruthy();
});

it("projects only the admitted server's pending or verified directory and preserves other servers", () => {
  const saved = { ...createFreshRoom(), serverId: "host", roomOrigin: "remote_server" as const, connectionState: "disconnected" as const };
  const other = { ...saved, serverId: "other" };
  const pending = projectRoomConnections([saved, other], null, true, [{ serverId: "host", state: "connecting" }]);
  expect(pending.map(room => room.connectionState)).toEqual(["connecting", "central-unconfirmed"]);
  expect(projectRoomConnections([saved], null, true, [{ serverId: "host", state: "connected" }])[0].connectionState).toBe("connected");
  expect(projectRoomConnections([saved], null, true, [{ serverId: "host", state: "disconnected" }])[0].connectionState).toBe("disconnected");
});

it("keeps a second live host connected while the first host drops", () => {
  const first = { ...createFreshRoom(), serverId: "first", roomOrigin: "remote_server" as const, connectionState: "connected" as const };
  const second = { ...first, serverId: "second" };
  const merged = projectRoomConnections([first, second], "connected", false, [
    { serverId: "first", state: "disconnected" },
    { serverId: "second", state: "connected" },
  ]);
  expect(merged.map(room => room.connectionState)).toEqual(["disconnected", "connected"]);
});
it("shows the new titles, no kicker and no raw origin text", () => {
  const [offline] = projectRoomConnections([{ ...createFreshRoom(), roomOrigin: "remote_server", connectionState: "disconnected" }], null, true);
  const { container } = render(<DisconnectedRoomView room={offline} onRetry={async () => {}} />);
  expect(screen.getByRole("heading", { name: `‘${offline.label}’에 연결할 수 없어요` })).toBeTruthy();
  expect(screen.queryByText("서버 연결 상태")).toBeNull();
  expect(container.querySelector("code")).toBeNull();
  expect(container.textContent).not.toContain("https://");
});
it("shows the connecting title while the host comes back", () => {
  const base = { ...createFreshRoom(), serverId: "host", roomOrigin: "remote_server" as const, connectionState: "disconnected" as const };
  const [connecting] = projectRoomConnections([base], null, false, [{ serverId: "host", state: "connecting" }]);
  render(<DisconnectedRoomView room={connecting} onRetry={async () => {}} />);
  expect(screen.getByRole("heading", { name: `‘${connecting.label}’에 연결하는 중이에요` })).toBeTruthy();
});
it("names the friend's computer when the host display name is known", () => {
  const base = { ...createFreshRoom(), serverId: "host", roomOrigin: "remote_server" as const, connectionState: "disconnected" as const };
  const [offline] = projectRoomConnections([base], null, false, [{ serverId: "host", state: "disconnected" }]);
  render(<DisconnectedRoomView room={offline} hostDisplayName="Hihi" onRetry={async () => {}} />);
  expect(screen.getByText(/Hihi님의 컴퓨터가 꺼져 있거나/)).toBeTruthy();
});
it("explains an unnamed remote server without leaking the origin", () => {
  const base = { ...createFreshRoom(), serverId: "host", roomOrigin: "remote_server" as const, connectionState: "disconnected" as const, serverOrigin: "https://friend.test" };
  const [offline] = projectRoomConnections([base], null, false, [{ serverId: "host", state: "disconnected" }]);
  const { container } = render(<DisconnectedRoomView room={offline} onRetry={async () => {}} />);
  expect(screen.getByText(/서버를 연 컴퓨터가 꺼져 있거나/)).toBeTruthy();
  expect(container.querySelector("code")).toBeNull();
});
it("keeps an explicit reason override and the retry button", async () => {
  const [offline] = projectRoomConnections([{ ...createFreshRoom(), roomOrigin: "remote_server", connectionState: "disconnected" }], null, true);
  const retry = vi.fn().mockResolvedValue(undefined);
  render(<DisconnectedRoomView room={offline} reason="참가 서버 정보가 바뀌었어요." onRetry={retry} />);
  expect(screen.getByText("참가 서버 정보가 바뀌었어요.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "다시 연결" }));
  await vi.waitFor(() => expect(retry).toHaveBeenCalledOnce());
});
it("waits for the daily reset without a retry button that cannot work", () => {
  const base = { ...createFreshRoom(), serverId: "host", roomOrigin: "remote_server" as const, connectionState: "disconnected" as const };
  const [offline] = projectRoomConnections([base], null, false, [{ serverId: "host", state: "disconnected" }]);
  render(<DisconnectedRoomView room={offline} reason="오늘 연결 한도를 다 써서 친구 방은 오전 9:00 이후에 다시 연결돼요." autoReconnectAt="오전 9:00" onRetry={async () => {}} />);
  expect(screen.getByText("오늘 연결 한도를 다 썼어요. 오전 9:00에 자동으로 다시 연결돼요.")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "다시 연결" })).toBeNull();
});
it("keeps the waiting title and no retry while connecting or waiting for the reset", () => {
  const base = { ...createFreshRoom(), serverId: "host", roomOrigin: "remote_server" as const, connectionState: "disconnected" as const };
  const [connecting] = projectRoomConnections([base], null, false, [{ serverId: "host", state: "connecting" }]);
  const { unmount } = render(<DisconnectedRoomView room={connecting} onRetry={async () => {}} />);
  expect(screen.queryByRole("button", { name: "다시 연결" })).toBeNull();
  unmount();
  render(<DisconnectedRoomView room={connecting} autoReconnectAt="오전 9:00" onRetry={async () => {}} />);
  expect(screen.getByRole("heading", { name: `‘${connecting.label}’에 연결할 수 없어요` })).toBeTruthy();
});
