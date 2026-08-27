const BROWSER_CREDENTIAL_STORAGE_KEY = "agentsassemble.browserCredential.v1";
const CLIENT_ID_STORAGE_KEY = "agentsassemble.clientId.v1";
const GUEST_PROFILE_STORAGE_KEY = "agentsassemble.guestProfile.v1";
const BROWSER_CREDENTIAL_PREFIX = "aad1_";
const BROWSER_CREDENTIAL_BYTES = 32;
const BROWSER_CREDENTIAL_BODY_CHARS = 43;
const BROWSER_CREDENTIAL_PATTERN = /^[A-Za-z0-9_-]{43}$/;

const BROWSER_CREDENTIAL_UNAVAILABLE =
  "이 브라우저에서 안전한 입장 자격 증명을 영구 저장할 수 없습니다.";
const BROWSER_CREDENTIAL_INVALID =
  "저장된 입장 자격 증명이 손상되었습니다. 브라우저 사이트 데이터를 확인해 주세요.";

export type RememberedGuestProfile = {
  displayName: string;
  avatarImage?: string;
};

function randomClientId(): string {
  try {
    if (typeof crypto !== "undefined" && crypto.randomUUID) {
      return crypto.randomUUID();
    }
  } catch {
    // Fall through to the manual generator on restricted webviews.
  }
  return `dev-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`;
}

function encodeBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

function isCanonicalBrowserCredential(value: string): boolean {
  if (!value.startsWith(BROWSER_CREDENTIAL_PREFIX)) return false;
  const encoded = value.slice(BROWSER_CREDENTIAL_PREFIX.length);
  if (
    encoded.length !== BROWSER_CREDENTIAL_BODY_CHARS ||
    !BROWSER_CREDENTIAL_PATTERN.test(encoded)
  ) {
    return false;
  }
  try {
    const binary = atob(encoded.replace(/-/g, "+").replace(/_/g, "/") + "=");
    const decoded = Uint8Array.from(binary, (character) => character.charCodeAt(0));
    return (
      decoded.length === BROWSER_CREDENTIAL_BYTES &&
      encodeBase64Url(decoded) === encoded
    );
  } catch {
    return false;
  }
}

/**
 * Returns the one durable credential used to bind browser admission and retry custody.
 *
 * This owner fails closed. It never imports the old device-token key and never
 * substitutes a weak or page-lifetime value when WebCrypto or durable storage fails.
 */
export function getOrCreateBrowserCredential(): string {
  if (typeof globalThis.crypto?.getRandomValues !== "function") {
    throw new Error(BROWSER_CREDENTIAL_UNAVAILABLE);
  }
  let storage: Storage;
  let existing: string | null;
  try {
    storage = window.localStorage;
    existing = storage.getItem(BROWSER_CREDENTIAL_STORAGE_KEY);
  } catch {
    throw new Error(BROWSER_CREDENTIAL_UNAVAILABLE);
  }
  if (existing !== null) {
    if (!isCanonicalBrowserCredential(existing)) {
      throw new Error(BROWSER_CREDENTIAL_INVALID);
    }
    return existing;
  }

  try {
    const bytes = new Uint8Array(BROWSER_CREDENTIAL_BYTES);
    globalThis.crypto.getRandomValues(bytes);
    const credential = `${BROWSER_CREDENTIAL_PREFIX}${encodeBase64Url(bytes)}`;
    storage.setItem(BROWSER_CREDENTIAL_STORAGE_KEY, credential);
    if (storage.getItem(BROWSER_CREDENTIAL_STORAGE_KEY) !== credential) {
      throw new Error(BROWSER_CREDENTIAL_UNAVAILABLE);
    }
    return credential;
  } catch {
    throw new Error(BROWSER_CREDENTIAL_UNAVAILABLE);
  }
}

export function getOrCreateClientId(): string {
  try {
    const existing = String(window.localStorage.getItem(CLIENT_ID_STORAGE_KEY) || "").trim();
    if (existing) return existing;
    const clientId = randomClientId();
    window.localStorage.setItem(CLIENT_ID_STORAGE_KEY, clientId);
    return clientId;
  } catch {
    return randomClientId();
  }
}

export function loadRememberedGuestProfile(): RememberedGuestProfile | null {
  try {
    const raw = window.localStorage.getItem(GUEST_PROFILE_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const displayName = String(parsed.displayName || "").trim();
    if (!displayName) return null;
    return {
      displayName,
      avatarImage: String(parsed.avatarImage || "").trim() || undefined,
    };
  } catch {
    return null;
  }
}

export function rememberGuestProfile(profile: RememberedGuestProfile) {
  try {
    if (!profile.displayName.trim()) return;
    window.localStorage.setItem(
      GUEST_PROFILE_STORAGE_KEY,
      JSON.stringify({
        displayName: profile.displayName.trim(),
        avatarImage: profile.avatarImage || "",
      })
    );
  } catch {
    // Best-effort: the join itself still works without remembering.
  }
}

export function clearRememberedGuestProfile() {
  try {
    window.localStorage.removeItem(GUEST_PROFILE_STORAGE_KEY);
  } catch {
    // The server-side identity switch is authoritative even when browser storage is unavailable.
  }
}
