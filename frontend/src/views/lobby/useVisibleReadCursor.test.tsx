import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useVisibleReadCursor } from "./useVisibleReadCursor";

afterEach(() => { cleanup(); vi.restoreAllMocks(); document.body.innerHTML = ""; });

function viewport() {
  const feed = document.createElement("div");
  document.body.append(feed);
  Object.defineProperties(feed, {
    clientHeight: { configurable: true, value: 400 },
    scrollHeight: { configurable: true, value: 1000 },
  });
  feed.scrollTop = 0;
  vi.spyOn(feed, "getBoundingClientRect").mockReturnValue({ left: 0, right: 400, bottom: 400 } as DOMRect);
  Object.defineProperty(document, "elementFromPoint", { configurable: true, value: vi.fn(() => feed) });
  return feed;
}

it("acknowledges a viewed latest sequence once and advances on a new visible message", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  const feed = viewport();
  const onMarkRead = vi.fn();
  const props = { roomId: "room-a", enabled: true, latestSequence: 12, lastReadSequence: 10,
    scrollRef: { current: feed }, onMarkRead };
  const { rerender } = renderHook(useVisibleReadCursor, { initialProps: props });
  expect(onMarkRead).not.toHaveBeenCalled();
  act(() => { feed.scrollTop = 560; feed.dispatchEvent(new Event("scroll")); });
  expect(onMarkRead).toHaveBeenLastCalledWith("seq:12");
  act(() => window.dispatchEvent(new Event("focus")));
  expect(onMarkRead).toHaveBeenCalledTimes(1);
  rerender({ ...props, latestSequence: 13, lastReadSequence: 12 });
  expect(onMarkRead).toHaveBeenLastCalledWith("seq:13");
  rerender({ ...props, roomId: "room-b" });
  expect(onMarkRead).toHaveBeenLastCalledWith("seq:12");
  expect(onMarkRead).toHaveBeenCalledTimes(3);
});

it("does not acknowledge hidden, unfocused, modal-covered, or historical feeds", () => {
  const focused = vi.spyOn(document, "hasFocus").mockReturnValue(false);
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  const feed = viewport(); feed.scrollTop = 600;
  const onMarkRead = vi.fn();
  const props = { roomId: "room-a", enabled: true, latestSequence: 12, lastReadSequence: 10,
    scrollRef: { current: feed }, onMarkRead };
  const { rerender } = renderHook(useVisibleReadCursor, { initialProps: props });
  expect(onMarkRead).not.toHaveBeenCalled();
  focused.mockReturnValue(true); visibility.mockReturnValue("hidden");
  act(() => window.dispatchEvent(new Event("focus")));
  expect(onMarkRead).not.toHaveBeenCalled();
  visibility.mockReturnValue("visible");
  const modal = document.createElement("div"); modal.setAttribute("aria-modal", "true"); document.body.append(modal);
  act(() => document.dispatchEvent(new Event("visibilitychange")));
  expect(onMarkRead).not.toHaveBeenCalled();
  modal.remove();
  vi.mocked(document.elementFromPoint).mockReturnValue(document.body);
  act(() => feed.dispatchEvent(new Event("scroll")));
  expect(onMarkRead).not.toHaveBeenCalled();
  vi.mocked(document.elementFromPoint).mockReturnValue(feed);
  rerender({ ...props, enabled: false });
  act(() => feed.dispatchEvent(new Event("scroll")));
  expect(onMarkRead).not.toHaveBeenCalled();
  rerender(props);
  expect(onMarkRead).toHaveBeenCalledExactlyOnceWith("seq:12");
});

it.each(["dialog", "section"])("rechecks a visible feed when a %s modal closes without scrolling", async (tag) => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  const feed = viewport(); feed.scrollTop = 560;
  const modal = document.createElement(tag);
  if (tag === "dialog") modal.setAttribute("open", "");
  else modal.setAttribute("aria-modal", "true");
  document.body.append(modal);
  const onMarkRead = vi.fn();
  renderHook(useVisibleReadCursor, { initialProps: {
    roomId: "room-a", enabled: true, latestSequence: 12, lastReadSequence: 10,
    scrollRef: { current: feed }, onMarkRead,
  } });
  expect(onMarkRead).not.toHaveBeenCalled();
  visibility.mockReturnValue("hidden");
  await act(async () => {
    if (tag === "dialog") modal.removeAttribute("open");
    else modal.remove();
  });
  expect(onMarkRead).not.toHaveBeenCalled();
  visibility.mockReturnValue("visible");
  if (tag === "dialog") modal.setAttribute("open", "");
  else document.body.append(modal);
  act(() => document.dispatchEvent(new Event("visibilitychange")));
  await act(async () => {
    if (tag === "dialog") modal.removeAttribute("open");
    else modal.remove();
  });
  expect(onMarkRead).toHaveBeenCalledExactlyOnceWith("seq:12");
  await act(async () => { document.body.append(document.createElement("span")); });
  act(() => window.dispatchEvent(new Event("focus")));
  expect(onMarkRead).toHaveBeenCalledTimes(1);
});
