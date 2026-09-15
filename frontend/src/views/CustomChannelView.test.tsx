import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
const api = vi.hoisted(() => ({ read: vi.fn(), write: vi.fn() }));
vi.mock("../api", async () => ({ ...(await vi.importActual("../api")), fetchMessagePins: api.read, setMessagePinned: api.write }));
import { RoomSocketSayError } from "../roomSocketTypes";
import { useChannelTranscript } from "../app/useChannelTranscript";
import type { RoomCommandAck, RoomSocketHandle } from "../roomSocketTypes";
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

it("retains exact channel retry until the draft changes", async () => {
  const options = props();
  const retry = vi.fn().mockResolvedValue({});
  vi.mocked(options.transcript.send)
    .mockRejectedValueOnce(new RoomSocketSayError("uncertain channel", "outcome_unknown", retry))
    .mockRejectedValueOnce(new Error("still offline"));
  render(<CustomChannelView {...options} />);
  const input = screen.getByRole("textbox", { name: "채널 메시지 입력" });
  fireEvent.change(input, { target: { value: "committed once" } });
  fireEvent.click(screen.getByRole("button", { name: "채널 메시지 보내기" }));
  await screen.findByText("uncertain channel");
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(options.transcript.send).toHaveBeenLastCalledWith("committed once", retry));
  await screen.findByText("still offline");
  fireEvent.change(input, { target: { value: "different intent" } });
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(options.transcript.send).toHaveBeenLastCalledWith("different intent"));
});


it.each(["reconnect", "channel", "room", "return"])("binds an in-flight receipt to its draft across %s", async (change) => {
  const options = props();
  let complete!: (value: RoomCommandAck) => void;
  const receipt = new Promise<RoomCommandAck>((resolve) => { complete = resolve; });
  const command = vi.fn((action: string) => action === "channel.message.send" ? receipt : Promise.resolve({
    result: { events: [], last_seq: 0, has_more_before: false },
  } as unknown as RoomCommandAck));
  const socket = { ready: () => true, command } as unknown as RoomSocketHandle;
  function View({ connected = true, selected = channelId, uid = "room-one" }) {
    const transcript = useChannelTranscript({ roomId: "general", roomUid: uid, channelId: selected, socket, connected });
    return <CustomChannelView {...options} roomUid={uid} channelId={selected} transcript={transcript} />;
  }
  const view = render(<View />);
  const input = screen.getByRole("textbox", { name: "채널 메시지 입력" }) as HTMLTextAreaElement;
  await waitFor(() => expect(input.disabled).toBe(false));
  fireEvent.change(input, { target: { value: "send once" } });
  fireEvent.keyDown(input, { key: "Enter" });
  view.rerender(<View connected={false} />);
  const selected = (change === "channel" || change === "return") ? "c111111111111" : channelId;
  const uid = change === "room" ? "room-two" : "room-one";
  view.rerender(<View selected={selected} uid={uid} />);
  await act(async () => {});
  if (change === "reconnect") {
    expect(input.disabled).toBe(true);
    expect(input.value).toBe("send once");
  } else {
    expect(input.disabled).toBe(false);
    if (change === "return") { view.rerender(<View />); await act(async () => {}); }
    fireEvent.change(input, { target: { value: "new draft" } });
  }
  await act(async () => complete({ accepted: true, resolution: "committed" } as RoomCommandAck));
  expect(input.value).toBe(change === "reconnect" ? "" : "new draft");
  expect(input.disabled).toBe(false);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(command.mock.calls.filter(([action]) => action === "channel.message.send")).toHaveLength(1);
});
