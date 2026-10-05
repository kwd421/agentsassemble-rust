import { ApiError } from "./apiErrors";
import { isPrivateNoStoreResponse, responseError } from "../api/http";
import { fetchDesktopOperatorRuntime } from "./desktopBridge";
import { parseOwnerSessionStatus } from "./central/ownerWorkspaceStatus";
import type { CentralOwnerSessionStatus } from "../types/generated/CentralOwnerSessionStatus";

export type OpenDirectoryStream = (signal: AbortSignal) => Promise<Response>;

export const openNativeDirectoryStream: OpenDirectoryStream = signal =>
  fetchDesktopOperatorRuntime("/api/rooms/events", {
    signal, cache: "no-store", credentials: "omit", redirect: "error", referrerPolicy: "no-referrer",
  }, () => signal.throwIfAborted());

// Only the server's fixed, empty invalidation is accepted. No directory data,
// credential, event cursor or client-selected authority is carried by this stream.
export async function readDirectoryStream(
  response: Response, signal: AbortSignal, changed: () => Promise<void>, ownerStatus?: (status: CentralOwnerSessionStatus) => void
) {
  if (!response.ok) throw await responseError(response);
  if (!isPrivateNoStoreResponse(response, "text/event-stream") || !response.body) {
    throw new Error("방 목록 변경 연결을 확인하지 못했어요.");
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let pending = "";
  // The server sends a transport comment every 15s. A missing transport for
  // 45s is a disconnect, rather than an indefinitely stale directory.
  let deadline = setTimeout(() => { void reader.cancel().catch(() => undefined); }, 45_000);
  const abort = () => { void reader.cancel().catch(() => undefined); };
  signal.addEventListener("abort", abort, { once: true });
  try {
    signal.throwIfAborted();
    while (!signal.aborted) {
      const { done, value } = await reader.read();
      if (done) throw new Error("방 목록 변경 연결이 끊겼어요.");
      clearTimeout(deadline);
      deadline = setTimeout(() => { void reader.cancel().catch(() => undefined); }, 45_000);
      pending += decoder.decode(value, { stream: true });
      let end = pending.indexOf("\n\n");
      while (end >= 0) {
        const frame = pending.slice(0, end);
        pending = pending.slice(end + 2);
        if (frame.length > 4096) throw new Error("방 목록 변경 알림이 너무 커요.");
        const fields = frame.split("\n").filter(line => !line.startsWith(":"));
        if (fields.length) {
          if (fields.length === 2 && fields[0] === "event: owner_session" && fields[1].startsWith("data: ") && ownerStatus) {
            const status = parseOwnerSessionStatus(JSON.parse(fields[1].slice(6)));
            signal.throwIfAborted();
            ownerStatus(status);
            if (status.state === "ended") throw new ApiError(401, "서버 연결이 종료됐어요.", "central_session_ended");
          } else if (fields.length === 2 && fields[0] === "event: directory_changed" && fields[1] === "data: {}") {
            signal.throwIfAborted();
            await changed();
          } else {
            throw new Error("방 목록 변경 알림이 올바르지 않아요.");
          }
        }
        end = pending.indexOf("\n\n");
      }
      if (pending.length > 4096) throw new Error("방 목록 변경 알림이 너무 커요.");
    }
  } finally {
    clearTimeout(deadline);
    signal.removeEventListener("abort", abort);
    await reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}

export function subscribeRoomDirectory(
  open: OpenDirectoryStream, changed: () => Promise<void>, failed: (error: unknown) => void,
  ownerStatus?: (status: CentralOwnerSessionStatus) => void,
  reconnectUntilClosed = false
) {
  const lifetime = new AbortController();
  let running = false;
  let terminal = false;
  const retry = () => {
    if (running || terminal || lifetime.signal.aborted) return;
    running = true;
    void (async () => {
      // Local hosts reconnect until closed, with a 30s delay cap. Web owner
      // workspaces retain four admissions per recovery attempt. Every
      // admitted connection starts with an invalidation to cover the missed gap.
      for (let attempt = 0; (reconnectUntilClosed || attempt < 4) && !lifetime.signal.aborted; attempt += 1) {
        const admission = new AbortController();
        const abort = () => admission.abort();
        lifetime.signal.addEventListener("abort", abort, { once: true });
        let headerDeadline: ReturnType<typeof setTimeout> | undefined;
        try {
          headerDeadline = setTimeout(abort, 10_000);
          const response = await open(admission.signal);
          clearTimeout(headerDeadline);
          await readDirectoryStream(response, admission.signal, changed, ownerStatus);
        } catch (error) {
          if (lifetime.signal.aborted) return;
          failed(error);
          if (ownerStatus && error instanceof ApiError && [401, 403].includes(error.status)) {
            terminal = true;
            ownerStatus({ state: "ended", reason: "disconnected" });
          }
          if ((!reconnectUntilClosed && attempt === 3) || error instanceof ApiError && [401, 403, 409, 429].includes(error.status)) return;
          await new Promise<void>(resolve => {
            const finish = () => {
              clearTimeout(timer);
              lifetime.signal.removeEventListener("abort", finish);
              resolve();
            };
            const timer = setTimeout(finish, Math.min(30_000, 500 * 2 ** Math.min(attempt, 6)));
            lifetime.signal.addEventListener("abort", finish, { once: true });
          });
        } finally {
          clearTimeout(headerDeadline);
          lifetime.signal.removeEventListener("abort", abort);
          admission.abort();
        }
      }
    })().finally(() => { running = false; });
  };
  // Network recovery starts one bounded burst; events during a burst coalesce.
  window.addEventListener("online", retry);
  retry();
  return { retry, close: () => {
    window.removeEventListener("online", retry);
    lifetime.abort();
  } };
}
