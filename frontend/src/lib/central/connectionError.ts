export class CentralTemporaryError extends Error {}
export function isCentralTemporaryError(error: unknown): boolean {
  return error instanceof CentralTemporaryError;
}
// Only the fetch boundary classifies network errors. Crypto/storage/parser errors
// must never turn into an offline admission.
export async function fetchCentral(input: RequestInfo | URL, init: RequestInit): Promise<Response> {
  try { return await fetch(input, init); }
  catch (error) {
    // WebKit may reject a timed-out fetch as AbortError ("Fetch is aborted").
    // The signal owns the distinction from user/unmount cancellation.
    if (init.signal?.aborted && init.signal.reason?.name !== "TimeoutError") throw error;
    if (init.signal?.reason?.name === "TimeoutError" || error instanceof TypeError || (error instanceof DOMException && error.name === "TimeoutError")) {
      throw new CentralTemporaryError("로그인 서버에 연결할 수 없어요.");
    }
    throw error;
  }
}
