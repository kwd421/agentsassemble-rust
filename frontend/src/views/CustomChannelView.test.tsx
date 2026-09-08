import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
const api = vi.hoisted(() => ({ read: vi.fn(), write: vi.fn() }));
vi.mock("../api", async () => ({ ...(await vi.importActual("../api")), fetchMessagePins: api.read, setMessagePinned: api.write }));
import CustomChannelView from "./CustomChannelView";
import { channelId, channelMessage } from "../test/channelMessage";

function props(): ComponentProps<typeof CustomChannelView> {
  return { channel: { id: channelId, name: "Notes", type: "text", position: 0, createdAt: "2026-09-08T00:00:00Z" },
    channelId, roomId: "general", roomUid: "room-one", canPost: true, canPin: true, authority: { kind: "local" },
    onOpenCrossChannelSearchResult: vi.fn(),
    transcript: { scope: { roomId: "general", roomUid: "room-one", channelId, socket: null, connected: true }, receive: vi.fn(), latest: vi.fn(), earlier: vi.fn(), showContext: vi.fn(), send: vi.fn().mockResolvedValue(undefined),
      events: [channelMessage(3)], hasMore: true, following: true, newMessages: false, ready: true, loading: false, sending: false, error: "" },
    messageSearch: { error: "", hasMore: false, loading: false, loadingMore: false, loadMore: vi.fn(), query: "", readContext: vi.fn(), results: [], setError: vi.fn(), updateQuery: vi.fn() },
  };
}
beforeEach(() => { Element.prototype.scrollIntoView = vi.fn(); api.read.mockResolvedValue([]); });
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("keeps failed text, clears only on the send receipt and restores input focus", async () => {
  const options = props();
  vi.mocked(options.transcript.send).mockRejectedValueOnce(new Error("send denied"));
  render(<CustomChannelView {...options} />);
  const input = screen.getByRole("textbox", { name: "채널 메시지 입력" }) as HTMLTextAreaElement;
  fireEvent.change(input, { target: { value: "retained text" } });
  fireEvent.click(screen.getByRole("button", { name: "채널 메시지 보내기" }));
  expect((await screen.findByRole("alert")).textContent).toContain("send denied");
  expect(input.value).toBe("retained text");
  fireEvent.click(screen.getByRole("button", { name: "채널 메시지 보내기" }));
  await waitFor(() => expect(input.value).toBe(""));
  expect(document.activeElement).toBe(input);
  expect(options.transcript.send).toHaveBeenLastCalledWith("retained text");
});

it("pins exact channel messages and opens older pins through their concrete context", async () => {
  const options = props();
  const older = channelMessage(1);
  const pin = { event_id: older.id, channel_id: channelId, seq: 1, pinned_at: older.created_at, created_at: older.created_at, author: "Host", content: "older pin", attachment_filenames: [] };
  api.write.mockResolvedValue([{ ...pin, event_id: "channel-event-3", seq: 3 }]); api.read.mockResolvedValue([pin]);
  vi.mocked(options.messageSearch.readContext).mockResolvedValue({ channel_id: channelId, event_id: older.id, events: [older] });
  render(<CustomChannelView {...options} />);
  fireEvent.click(screen.getByRole("button", { name: "메시지 고정" }));
  await waitFor(() => expect(api.write).toHaveBeenCalledWith(expect.objectContaining({ channelId, roomId: "general", eventId: "channel-event-3", pinned: true })));
  fireEvent.click(screen.getByRole("button", { name: "고정 메시지" }));
  fireEvent.click(await screen.findByRole("button", { name: /Host.*older pin/ }));
  await waitFor(() => expect(options.messageSearch.readContext).toHaveBeenCalledWith(older.id, channelId));
  expect(options.transcript.showContext).toHaveBeenCalledWith([older]);
});

it("keeps read-only controls disabled and offers explicit older/latest navigation", async () => {
  const options = props();
  options.canPost = false; options.canPin = false;
  options.transcript.following = false; options.transcript.newMessages = true;
  render(<CustomChannelView {...options} />);
  expect((screen.getByRole("textbox", { name: "채널 메시지 입력" }) as HTMLTextAreaElement).disabled).toBe(true);
  expect(screen.queryByRole("button", { name: "메시지 고정" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "이전 메시지" }));
  expect(options.transcript.earlier).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "새 메시지 · 최신으로" }));
  expect(options.transcript.latest).toHaveBeenCalledOnce();
});

it("preserves a draft across a connection interruption and rejects stale context publication", async () => {
  const options = props();
  let complete!: (value: Awaited<ReturnType<typeof options.messageSearch.readContext>>) => void;
  vi.mocked(options.messageSearch.readContext).mockImplementation(() => new Promise((resolve) => { complete = resolve; }));
  const { rerender } = render(<CustomChannelView {...options} pendingSearchTargetEventId="channel-event-1" />);
  const input = screen.getByRole("textbox", { name: "채널 메시지 입력" }) as HTMLTextAreaElement;
  fireEvent.change(input, { target: { value: "keep through reconnect" } });
  rerender(<CustomChannelView {...options} channel={null} transcript={{ ...options.transcript, scope: { ...options.transcript.scope, connected: false }, ready: false, events: [] }} />);
  expect(input.value).toBe("keep through reconnect");
  expect(input.disabled).toBe(true);
  await act(async () => complete({ channel_id: channelId, event_id: "channel-event-1", events: [channelMessage(1)] }));
  expect(options.transcript.showContext).not.toHaveBeenCalled();
});
