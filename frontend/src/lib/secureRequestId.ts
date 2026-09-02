const REQUEST_ID_UNAVAILABLE = "Secure request identity is unavailable.";

/** Creates the browser-owned identity for one replayable request. */
export function createSecureRequestId(): string {
  const randomUuid = globalThis.crypto?.randomUUID;
  if (typeof randomUuid !== "function") {
    throw new Error(REQUEST_ID_UNAVAILABLE);
  }
  try {
    return randomUuid.call(globalThis.crypto);
  } catch {
    throw new Error(REQUEST_ID_UNAVAILABLE);
  }
}
