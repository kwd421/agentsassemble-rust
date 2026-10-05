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
  expect(screen.getByText(offline.label)).toBeTruthy();
});
