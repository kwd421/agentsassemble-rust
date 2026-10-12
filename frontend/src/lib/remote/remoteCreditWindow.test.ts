import { afterEach, expect, test, vi } from "vitest";
import { CreditWindow } from "./remoteCreditWindow";
afterEach(() => vi.useRealTimers());

test("credit for B cannot extend A's oldest emitted record deadline", async () => {
  vi.useFakeTimers(); const timeout = vi.fn(); const window = new CreditWindow(timeout);
  await window.reserve(1, 4); window.emitted(1, 4);
  await vi.advanceTimersByTimeAsync(10_000);
  await window.reserve(2, 4); window.emitted(2, 4);
  await vi.advanceTimersByTimeAsync(19_000); window.credit(2, 4);
  await vi.advanceTimersByTimeAsync(999); expect(timeout).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1); expect(timeout).toHaveBeenCalledTimes(1); window.close();
});

test("reservation backlog costs no peer time; partial credit advances to the next emission, release removes it", async () => {
  vi.useFakeTimers(); const timeout = vi.fn(); const window = new CreditWindow(timeout);
  await window.reserve(1, 4);
  await vi.advanceTimersByTimeAsync(60_000); expect(timeout).not.toHaveBeenCalled();
  window.emitted(1, 4); await vi.advanceTimersByTimeAsync(10_000);
  await window.reserve(1, 4); window.emitted(1, 8);
  await vi.advanceTimersByTimeAsync(19_000); window.credit(1, 4);
  await vi.advanceTimersByTimeAsync(10_000); expect(timeout).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1_000); expect(timeout).toHaveBeenCalledTimes(1);
  window.release(1); await vi.advanceTimersByTimeAsync(60_000); expect(timeout).toHaveBeenCalledTimes(1); window.close();
});
