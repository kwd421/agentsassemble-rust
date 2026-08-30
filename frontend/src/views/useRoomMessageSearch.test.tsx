import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { MessageSearchAuthority, RoomSearchPage } from "../api";

const api = vi.hoisted(() => ({
  context: vi.fn(),
  search: vi.fn(),
}));

vi.mock("../api", async () => ({
  ...(await vi.importActual<typeof import("../api")>("../api")),
  fetchRoomMessageContext: api.context,
  searchRoomMessages: api.search,
}));

import { useRoomMessageSearch } from "./useRoomMessageSearch";

const page: RoomSearchPage = {
  results: [{
    event_id: "event-1",
    channel_id: "lobby",
    seq: 1,
    created_at: "2026-08-30T01:00:00Z",
    author: "Operator",
    content: "private room history",
    attachment_filenames: [],
  }],
  next_cursor: "",
};

function renderSearch(authority?: MessageSearchAuthority) {
  return renderHook(
    ({ currentAuthority }: { currentAuthority?: MessageSearchAuthority }) =>
      useRoomMessageSearch({
        roomId: "general",
        channelId: "lobby",
        authority: currentAuthority,
      }),
    { initialProps: { currentAuthority: authority } }
  );
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.useRealTimers();
});

describe("useRoomMessageSearch authority lifecycle", () => {
  it("drops visible search state when authority is removed", async () => {
    vi.useFakeTimers();
    api.search.mockResolvedValueOnce(page);
    const hook = renderSearch({ kind: "remote", sessionToken: "session-a" });

    act(() => hook.result.current.updateQuery("history"));
    await act(() => vi.advanceTimersByTimeAsync(250));
    expect(hook.result.current.results).toEqual(page.results);

    hook.rerender({ currentAuthority: undefined });

    expect(hook.result.current.query).toBe("");
    expect(hook.result.current.results).toEqual([]);
    expect(hook.result.current.hasMore).toBe(false);
  });

  it("ignores a response completed after the authority changes", async () => {
    vi.useFakeTimers();
    let resolveSearch: (value: RoomSearchPage) => void = () => undefined;
    api.search.mockReturnValueOnce(new Promise<RoomSearchPage>((resolve) => {
      resolveSearch = resolve;
    }));
    const hook = renderSearch({ kind: "remote", sessionToken: "session-a" });

    act(() => hook.result.current.updateQuery("history"));
    await act(() => vi.advanceTimersByTimeAsync(250));
    hook.rerender({
      currentAuthority: { kind: "remote", sessionToken: "session-b" },
    });
    await act(async () => resolveSearch(page));

    expect(hook.result.current.query).toBe("");
    expect(hook.result.current.results).toEqual([]);
  });
});
