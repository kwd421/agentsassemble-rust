import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  MessageSearchAuthority,
  RoomMessageContext,
  RoomSearchPage,
} from "../api";

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
    participant_id: "operator-local",
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

  it("does not return delayed context after the authority changes", async () => {
    let resolveContext: (value: RoomMessageContext) => void = () => undefined;
    api.context.mockReturnValueOnce(new Promise<RoomMessageContext>((resolve) => {
      resolveContext = resolve;
    }));
    const hook = renderSearch({ kind: "remote", sessionToken: "session-a" });
    const context = hook.result.current.readContext("event-1");

    hook.rerender({
      currentAuthority: { kind: "remote", sessionToken: "session-b" },
    });
    await act(async () => resolveContext({
      channel_id: "lobby",
      event_id: "event-1",
      events: [],
    }));

    await expect(context).resolves.toBeNull();
  });

  it("does not return a delayed context error after the authority changes", async () => {
    let rejectContext: (reason: Error) => void = () => undefined;
    api.context.mockReturnValueOnce(new Promise<RoomMessageContext>((_resolve, reject) => {
      rejectContext = reject;
    }));
    const hook = renderSearch({ kind: "remote", sessionToken: "session-a" });
    const context = hook.result.current.readContext("event-1");

    hook.rerender({
      currentAuthority: { kind: "remote", sessionToken: "session-b" },
    });
    await act(async () => rejectContext(new Error("old authority failed")));

    await expect(context).resolves.toBeNull();
  });

  it("keeps the latest context selection when an earlier success arrives last", async () => {
    const resolvers: Array<(value: RoomMessageContext) => void> = [];
    api.context.mockImplementation(() => new Promise<RoomMessageContext>((resolve) => {
      resolvers.push(resolve);
    }));
    const hook = renderSearch({ kind: "remote", sessionToken: "session-a" });
    const first = hook.result.current.readContext("event-1");
    const second = hook.result.current.readContext("event-2");
    const secondContext: RoomMessageContext = {
      channel_id: "lobby",
      event_id: "event-2",
      events: [],
    };

    await act(async () => resolvers[1](secondContext));
    await act(async () => resolvers[0]({
      channel_id: "lobby",
      event_id: "event-1",
      events: [],
    }));

    await expect(second).resolves.toEqual(secondContext);
    await expect(first).resolves.toBeNull();
  });

  it("discards an earlier context error after a later selection succeeds", async () => {
    let rejectFirst: (reason: Error) => void = () => undefined;
    api.context
      .mockReturnValueOnce(new Promise<RoomMessageContext>((_resolve, reject) => {
        rejectFirst = reject;
      }))
      .mockResolvedValueOnce({
        channel_id: "lobby",
        event_id: "event-2",
        events: [],
      });
    const hook = renderSearch({ kind: "remote", sessionToken: "session-a" });
    const first = hook.result.current.readContext("event-1");
    const second = hook.result.current.readContext("event-2");

    await expect(second).resolves.toMatchObject({ event_id: "event-2" });
    await act(async () => rejectFirst(new Error("earlier selection failed")));

    await expect(first).resolves.toBeNull();
  });

  it("releases pagination loading when the query changes", async () => {
    vi.useFakeTimers();
    api.search.mockResolvedValueOnce({ ...page, next_cursor: "cursor-1" });
    const hook = renderSearch({ kind: "remote", sessionToken: "session-a" });

    act(() => hook.result.current.updateQuery("history"));
    await act(() => vi.advanceTimersByTimeAsync(250));
    let resolveMore: (value: RoomSearchPage) => void = () => undefined;
    api.search.mockReturnValueOnce(new Promise<RoomSearchPage>((resolve) => {
      resolveMore = resolve;
    }));
    let loadMore: Promise<void> = Promise.resolve();
    await act(async () => {
      loadMore = hook.result.current.loadMore();
      await Promise.resolve();
    });
    expect(hook.result.current.loadingMore).toBe(true);

    act(() => hook.result.current.updateQuery("new query"));
    expect(hook.result.current.loadingMore).toBe(false);
    await act(async () => resolveMore({ ...page, results: [], next_cursor: "" }));
    await loadMore;

    expect(hook.result.current.loadingMore).toBe(false);
  });
});
