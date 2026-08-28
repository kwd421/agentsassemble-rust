import { ApiError } from "../lib/apiErrors";
import {
  fetchDesktopOperatorRuntime,
  fetchDesktopRuntime,
  isDesktopWebview,
} from "../lib/desktopBridge";

const HOST_TOKEN_STORAGE_KEY = "agentsassemble.hostToken.v1";
let inMemoryHostToken = "";

function isServerWideProfileRoute(url: string): boolean {
  const path = url.split("?", 1)[0];
  return path === "/api/user-profile" || path === "/api/attachments";
}

export async function exchangeSessionTicket(
  purpose: "profile" | "socket" | "preferences-read" | "preferences-write",
  sessionToken: string
): Promise<Record<string, unknown>> {
  const res = await fetch(`/api/session-tickets/${purpose}`, {
    cache: "no-store",
    method: "POST",
    headers: { Authorization: `Bearer ${sessionToken}` },
  });
  if (!res.ok) throw await responseError(res);
  const payload = await res.json();
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    throw new Error("Session ticket response is invalid.");
  }
  return payload as Record<string, unknown>;
}

export async function exchangeSessionHttpTicket(
  purpose: "profile" | "preferences-read" | "preferences-write",
  sessionToken: string
): Promise<string> {
  const payload = await exchangeSessionTicket(purpose, sessionToken);
  const ticket = typeof payload.ticket === "string" ? payload.ticket : "";
  if (!/^[0-9a-f]{64}$/.test(ticket)) {
    throw new Error("Session HTTP ticket response is invalid.");
  }
  return ticket;
}

async function profileTargetToken(url: string, sessionToken: string): Promise<string> {
  return sessionToken && isServerWideProfileRoute(url)
    ? exchangeSessionHttpTicket("profile", sessionToken)
    : sessionToken;
}

export function loadHostToken(): string {
  try {
    return String(sessionStorage.getItem(HOST_TOKEN_STORAGE_KEY) || inMemoryHostToken || "").trim();
  } catch {
    return inMemoryHostToken;
  }
}

export function saveHostToken(token: string) {
  const cleanToken = String(token || "").trim();
  inMemoryHostToken = cleanToken;
  try {
    if (cleanToken) {
      sessionStorage.setItem(HOST_TOKEN_STORAGE_KEY, cleanToken);
    } else {
      sessionStorage.removeItem(HOST_TOKEN_STORAGE_KEY);
    }
  } catch {
    // Session storage can be unavailable in restricted browser contexts.
  }
}

export function clearHostToken() {
  saveHostToken("");
}

export async function fetchJson<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) throw await responseError(res);
  return res.json();
}

export async function postJson<T>(url: string, body: object): Promise<T> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    throw await responseError(res);
  }
  return res.json();
}

export async function fetchJsonServerOperator<T>(
  url: string,
  beforeDispatch?: () => void
): Promise<T> {
  if (isDesktopWebview()) {
    const res = await fetchDesktopOperatorRuntime(url, {}, beforeDispatch);
    if (!res.ok) throw await responseError(res);
    return res.json();
  }
  beforeDispatch?.();
  return fetchJson<T>(url);
}

export async function postEmptyServerOperator<T>(
  url: string,
  beforeDispatch?: () => void
): Promise<T> {
  let res: Response;
  if (isDesktopWebview()) {
    res = await fetchDesktopOperatorRuntime(url, { method: "POST" }, beforeDispatch);
  } else {
    beforeDispatch?.();
    res = await fetch(url, { method: "POST" });
  }
  if (!res.ok) throw await responseError(res);
  return res.json();
}

export async function postJsonServerOperator<T>(url: string, body: object): Promise<T> {
  if (isDesktopWebview()) {
    const res = await fetchDesktopOperatorRuntime(url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) throw await responseError(res);
    return res.json();
  }
  return postJson<T>(url, body);
}

export async function postJsonHost<T>(url: string, body: object): Promise<T> {
  const hostToken = loadHostToken();
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (hostToken) headers["X-Host-Token"] = hostToken;
  const res = await fetch(url, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    throw await responseError(res);
  }
  return res.json();
}

export async function postJsonModerator<T>(url: string, body: object, sessionToken = ""): Promise<T> {
  // Moderation endpoints accept the host token (local console) or the
  // operator's guest session token (public entrance) — send what we have.
  const hostToken = loadHostToken();
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (hostToken) headers["X-Host-Token"] = hostToken;
  if (sessionToken) headers["Authorization"] = `Bearer ${sessionToken}`;
  const res = await fetch(url, { method: "POST", headers, body: JSON.stringify(body) });
  if (!res.ok) {
    throw await responseError(res);
  }
  return res.json();
}

export async function fetchJsonWithToken<T>(url: string, sessionToken: string): Promise<T> {
  const res = await fetch(url, {
    headers: { Authorization: `Bearer ${sessionToken}` },
  });
  if (!res.ok) throw await responseError(res);
  return res.json();
}

export async function postJsonWithToken<T>(url: string, body: object, sessionToken: string): Promise<T> {
  const res = await fetch(url, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${sessionToken}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    throw await responseError(res);
  }
  return res.json();
}

export async function fetchJsonWithIdentity<T>(
  url: string,
  { sessionToken = "", deviceToken = "", roomId = "" }: { sessionToken?: string; deviceToken?: string; roomId?: string }
): Promise<T> {
  const profileRequest = isServerWideProfileRoute(url) ? { cache: "no-store" as const } : {};
  if (!sessionToken && isDesktopWebview() && isServerWideProfileRoute(url)) {
    const res = await fetchDesktopOperatorRuntime(url, profileRequest);
    if (!res.ok) throw await responseError(res);
    return res.json();
  }
  if (roomId && !sessionToken && isDesktopWebview()) {
    const res = await fetchDesktopRuntime(roomId, url, profileRequest);
    if (!res.ok) throw await responseError(res);
    return res.json();
  }
  const targetToken = await profileTargetToken(url, sessionToken);
  const headers: Record<string, string> = {};
  if (targetToken) headers.Authorization = `Bearer ${targetToken}`;
  if (deviceToken) headers["X-Device-Token"] = deviceToken;
  const res = await fetch(url, { ...profileRequest, headers });
  if (!res.ok) throw await responseError(res);
  return res.json();
}

export async function postJsonWithIdentity<T>(
  url: string,
  body: object,
  { sessionToken = "", deviceToken = "", roomId = "" }: { sessionToken?: string; deviceToken?: string; roomId?: string }
): Promise<T> {
  const profileRequest = isServerWideProfileRoute(url) ? { cache: "no-store" as const } : {};
  if (!sessionToken && isDesktopWebview() && isServerWideProfileRoute(url)) {
    const res = await fetchDesktopOperatorRuntime(url, {
      ...profileRequest,
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) throw await responseError(res);
    return res.json();
  }
  if (roomId && !sessionToken && isDesktopWebview()) {
    const res = await fetchDesktopRuntime(roomId, url, {
      ...profileRequest,
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) throw await responseError(res);
    return res.json();
  }
  const targetToken = await profileTargetToken(url, sessionToken);
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (targetToken) headers.Authorization = `Bearer ${targetToken}`;
  if (deviceToken) headers["X-Device-Token"] = deviceToken;
  const res = await fetch(url, {
    ...profileRequest,
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  if (!res.ok) throw await responseError(res);
  return res.json();
}

export async function deleteJsonWithIdentity<T>(
  url: string,
  { sessionToken = "", deviceToken = "" }: { sessionToken?: string; deviceToken?: string }
): Promise<T> {
  const headers: Record<string, string> = {};
  if (sessionToken) headers.Authorization = `Bearer ${sessionToken}`;
  if (deviceToken) headers["X-Device-Token"] = deviceToken;
  const res = await fetch(url, { method: "DELETE", headers });
  if (!res.ok) throw await responseError(res);
  return res.json();
}

export async function deleteJson<T>(url: string): Promise<T> {
  const res = await fetch(url, { method: "DELETE" });
  if (!res.ok) {
    throw await responseError(res);
  }
  return res.json();
}

export async function responseError(res: Response): Promise<ApiError> {
  const fallback = `${res.status} ${res.statusText}`;
  const text = await res.text().catch(() => "");
  if (!text) return new ApiError(res.status, fallback);
  try {
    const payload = JSON.parse(text) as {
      code?: unknown;
      error?: unknown;
      message?: unknown;
    };
    const nestedError =
      typeof payload.error === "object" &&
      payload.error !== null &&
      !Array.isArray(payload.error)
        ? (payload.error as { code?: unknown; message?: unknown })
        : null;
    const nestedMessage =
      typeof nestedError?.message === "string" && nestedError.message.trim()
        ? nestedError.message
        : "";
    const message =
      nestedMessage ||
      (typeof payload.error === "string" && payload.error.trim()
        ? payload.error
        : typeof payload.message === "string" && payload.message.trim()
          ? payload.message
          : fallback);
    const codeSource = nestedMessage ? nestedError?.code : payload.code;
    const code = typeof codeSource === "string" ? codeSource.trim() : "";
    return new ApiError(
      res.status,
      message,
      code
    );
  } catch {
    return new ApiError(res.status, text);
  }
}

export function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => {
      const result = String(reader.result || "");
      resolve(result.includes(",") ? result.split(",", 2)[1] : result);
    });
    reader.addEventListener("error", () => reject(reader.error || new Error("file read failed")));
    reader.readAsDataURL(file);
  });
}

export function queryString(params: Record<string, string | undefined>) {
  const query = new URLSearchParams();
  Object.entries(params).forEach(([key, value]) => {
    if (value) query.set(key, value);
  });
  const text = query.toString();
  return text ? `?${text}` : "";
}
