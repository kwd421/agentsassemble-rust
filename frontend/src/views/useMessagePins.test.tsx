import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { MessagePin } from "../api";
const api = vi.hoisted(() => ({ read: vi.fn(), write: vi.fn() }));
vi.mock("../api", async () => ({ ...(await vi.importActual("../api")), fetchMessagePins: api.read, setMessagePinned: api.write }));
import { useMessagePins } from "./useMessagePins";

afterEach(() => { cleanup(); vi.resetAllMocks(); });
const first: Parameters<typeof useMessagePins>[0] = { roomId: "general", roomUid: "room-one", channelId: "c0123456789ab", authority: { kind: "remote", sessionToken: "aas1.first" } } as const;
const pin: MessagePin = { event_id: "event-one", channel_id: first.channelId, seq: 1,
  pinned_at: "2026-09-08T00:00:00Z", created_at: "2026-09-08T00:00:00Z", author: "Human", content: "pinned", attachment_filenames: [] };

it.each([
  { ...first, channelId: "c0123456789ac" },
  { ...first, roomUid: "recreated-room" },
  { ...first, authority: { kind: "remote", sessionToken: "aas1.second" } },
] as const)("fences a late mutation when channel, room incarnation or login changes", async (next) => {
  let complete!: (pins: MessagePin[]) => void;
  api.write.mockImplementationOnce(() => new Promise<MessagePin[]>((resolve) => { complete = resolve; }));
  api.read.mockResolvedValue([]);
  const hook = renderHook((options: Parameters<typeof useMessagePins>[0]) => useMessagePins(options), { initialProps: first });
  let pending!: Promise<void>;
  act(() => { pending = hook.result.current.setPinned(pin.event_id, true); });
  expect(hook.result.current.pinBusyIds.has(pin.event_id)).toBe(true);
  hook.rerender(next);
  expect(hook.result.current.pinnedItems).toEqual([]);
  expect(hook.result.current.pinBusyIds.size).toBe(0);
  await act(() => hook.result.current.reloadPins());
  expect(api.read).toHaveBeenCalledWith(expect.objectContaining({ channelId: next.channelId, authority: next.authority }));
  await act(async () => { complete([pin]); await pending; });
  expect(hook.result.current.pinnedItems).toEqual([]);
  expect(hook.result.current.pinsError).toBe("");
});
