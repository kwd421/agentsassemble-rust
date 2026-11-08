// @vitest-environment node
import { afterEach, expect, test, vi } from "vitest";
import { isHostUnreachable, RemoteTransport } from "./remoteTransport";
vi.hoisted(() => Object.defineProperty(globalThis, "HTMLDialogElement", { configurable: true, value: class {} }));
vi.mock("./secureCrypto", () => ({
  beginSecureHandshake: async () => ({ hello: {}, finish: async () => { throw new Error("unreachable in this test"); } }),
}));

class Socket {
  static OPEN = 1;
  static opens = false;
  readyState = 0;
  onopen?: () => void; onmessage?: () => void; onerror?: () => void; onclose?: () => void;
  constructor() {
    queueMicrotask(() => {
      if (Socket.opens) { this.readyState = 1; this.onopen?.(); queueMicrotask(() => this.onclose?.()); }
      else this.onerror?.();
    });
  }
  send() {}
  close() {}
}

afterEach(() => vi.unstubAllGlobals());

test("a host that never answers is unreachable, not a connection that ended", async () => {
  vi.stubGlobal("WebSocket", Socket);
  Socket.opens = false;
  const refused = await RemoteTransport.connect({ origin: "https://host.test" } as never, "owner").catch((error: unknown) => error);
  expect(isHostUnreachable(refused)).toBe(true);
  expect((refused as Error).message).toBe("서버 컴퓨터에 연결하지 못했어요. 그 컴퓨터가 꺼져 있거나 앱이 실행 중이 아닐 수 있어요.");

  Socket.opens = true;
  const ended = await RemoteTransport.connect({ origin: "https://host.test" } as never, "owner").catch((error: unknown) => error);
  expect(isHostUnreachable(ended)).toBe(false);
  expect((ended as Error).message).toBe("연결이 끝났어요. 서버 목록에서 다시 연결해 주세요.");
});
