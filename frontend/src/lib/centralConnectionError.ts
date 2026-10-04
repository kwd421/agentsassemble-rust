export class CentralTemporaryError extends Error {}
export function isCentralTemporaryError(error: unknown): boolean {
  return error instanceof CentralTemporaryError;
}
// Only the fetch boundary classifies network errors. Crypto/storage/parser errors
// must never turn into an offline admission.
export async function fetchCentral(input: RequestInfo | URL, init: RequestInit): Promise<Response> {
  try { return await fetch(input, init); }
  catch (error) {
    if (error instanceof TypeError || (error instanceof DOMException && error.name === "TimeoutError")) {
      throw new CentralTemporaryError("중앙 서버에 연결할 수 없어요.");
    }
    throw error;
  }
}
