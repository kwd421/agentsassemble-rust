export const ADMISSION_REQUEST_ID_STORAGE_KEY =
  "agentsassemble.roomAdmissionRequestId.v1";

const ADMISSION_REQUEST_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const NIL_REQUEST_ID = "00000000-0000-0000-0000-000000000000";
const REQUEST_ID_UNAVAILABLE_MESSAGE =
  "이 브라우저에서는 안전한 입장 요청 ID를 영구 저장할 수 없습니다.";

function requestIdIsCanonical(value: string): boolean {
  return value !== NIL_REQUEST_ID && ADMISSION_REQUEST_ID_PATTERN.test(value);
}

export function loadOrCreateAdmissionRequestId(): string {
  if (typeof globalThis.crypto?.randomUUID !== "function") {
    throw new Error(REQUEST_ID_UNAVAILABLE_MESSAGE);
  }
  let storage: Storage;
  let existing: string | null;
  try {
    storage = window.sessionStorage;
    existing = storage.getItem(ADMISSION_REQUEST_ID_STORAGE_KEY);
  } catch {
    throw new Error(REQUEST_ID_UNAVAILABLE_MESSAGE);
  }
  if (existing !== null) {
    if (!requestIdIsCanonical(existing)) {
      throw new Error(REQUEST_ID_UNAVAILABLE_MESSAGE);
    }
    return existing;
  }
  try {
    const requestId = globalThis.crypto.randomUUID();
    if (!requestIdIsCanonical(requestId)) {
      throw new Error(REQUEST_ID_UNAVAILABLE_MESSAGE);
    }
    storage.setItem(ADMISSION_REQUEST_ID_STORAGE_KEY, requestId);
    if (storage.getItem(ADMISSION_REQUEST_ID_STORAGE_KEY) !== requestId) {
      throw new Error(REQUEST_ID_UNAVAILABLE_MESSAGE);
    }
    return requestId;
  } catch {
    throw new Error(REQUEST_ID_UNAVAILABLE_MESSAGE);
  }
}

export function clearAdmissionRequestId(): void {
  try {
    window.sessionStorage.removeItem(ADMISSION_REQUEST_ID_STORAGE_KEY);
  } catch {
    // The completed room session is authoritative even if storage cleanup fails.
  }
}
