import { afterEach, expect, it, vi } from "vitest";
import { isCentralTemporaryError, fetchCentral } from "./connectionError";
import { unsignedPost, isCentralAuthenticationError } from "./identity";
afterEach(() => vi.unstubAllGlobals());
it.each([429, 500, 502, 503, 504])("classifies HTTP %s as transient, never authentication", async status => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("unavailable", { status })));
  const error = await unsignedPost("/test", {}).catch(e => e);
  expect(isCentralTemporaryError(error)).toBe(true);
  expect(isCentralAuthenticationError(error)).toBe(false);
});
it.each([400, 401, 403, 404, 409])("does not treat HTTP %s as an outage", async status => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("rejected", { status })));
  const error = await unsignedPost("/test", {}).catch(e => e);
  expect(isCentralTemporaryError(error)).toBe(false);
  expect(isCentralAuthenticationError(error)).toBe(status === 401);
});
it("classifies network/timeout at fetch only and preserves cancellation", async () => {
  for (const error of [new TypeError("fetch failed"), new DOMException("timeout", "TimeoutError")]) {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(error));
    expect(isCentralTemporaryError(await fetchCentral("/test", {}).catch(e => e))).toBe(true);
  }
  const abort = new DOMException("cancelled", "AbortError");
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(abort));
  await expect(fetchCentral("/test", {})).rejects.toBe(abort);
  expect(isCentralTemporaryError(new TypeError("parser failure"))).toBe(false);
});

it.each([null, 0, "unavailable"])("uses HTTP 401 even when its JSON error body is %s", async payload => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(Response.json(payload, { status: 401 })));
  const error = await unsignedPost("/test", {}).catch(e => e);
  expect(isCentralAuthenticationError(error)).toBe(true);
  expect(isCentralTemporaryError(error)).toBe(false);
});

it("uses the timeout signal when WebKit reports an AbortError and preserves caller cancellation", async () => {
  const error = new DOMException("Fetch is aborted", "AbortError");
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(error));
  const timeout = AbortSignal.abort(new DOMException("Timed out", "TimeoutError"));
  expect(isCentralTemporaryError(await fetchCentral("/test", { signal: timeout }).catch(e => e))).toBe(true);
  await expect(fetchCentral("/test", { signal: AbortSignal.abort() })).rejects.toBe(error);
});
