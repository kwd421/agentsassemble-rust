import { decodeCanonicalBase64Url, encodeBase64Url } from "../base64Url.ts";
import { beginSecureHandshake, type SecureTarget, type SecurePurpose, type ServerHello, type SecureCipher } from "./secureCrypto.ts";

export const REMOTE_CHUNK = 64 * 1024;
const QUEUE_LIMIT = 512 * 1024;
const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
export const connectionEnded = () => new Error("연결이 끝났어요. 서버 목록에서 다시 연결해 주세요.");
const capacityError = () => new Error("전송할 내용이 많아요. 잠시 후 다시 시도해 주세요.");
export type RoomSocketConnection = {
  readonly readyState: number;
  onopen: ((event: Event) => void) | null;
  onmessage: ((event: MessageEvent) => void) | null;
  onerror: ((event: Event) => void) | null;
  onclose: ((event: CloseEvent) => void) | null;
  send(data: string): void;
  close(): void;
};
type HttpWait = {
  resolve: (response: Response) => void; reject: (error: Error) => void;
  controller?: ReadableStreamDefaultController<Uint8Array<ArrayBuffer>>;
  chunks: Uint8Array<ArrayBuffer>[]; end: boolean; pulling: boolean;
  removeAbort: () => void; responded: boolean; releaseFrame?: () => void;
};

function exact(frame: Record<string, unknown>, keys: string[]) {
  if (Object.keys(frame).sort().join() !== keys.sort().join()) throw connectionEnded();
}

export class RemoteTransport {
  readonly hello: ServerHello;
  private wire: WebSocket;
  private sendCipher: SecureCipher;
  private receiveCipher: SecureCipher;
  private closed = false;
  private nextId = 1;
  private sending = Promise.resolve();
  private receiving = Promise.resolve();
  private sendBytes = 0;
  private receiveBytes = 0;
  private bodyBytes = 0;
  private acknowledged = 0;
  private http = new Map<number, HttpWait>();
  private sockets = new Map<number, RemoteSocket>();
  private listeners = new Set<() => void>();
  private ready: { resolve: () => void; reject: (error: Error) => void } | null = null;

  private constructor(wire: WebSocket, agreement: { hello: ServerHello; send: SecureCipher; receive: SecureCipher }) {
    this.wire = wire; this.hello = agreement.hello; this.sendCipher = agreement.send; this.receiveCipher = agreement.receive;
    wire.onmessage = event => {
      if (!(event.data instanceof ArrayBuffer) || event.data.byteLength > 96 * 1024 + 24) { this.close(); return; }
      const record = event.data;
      this.receiveBytes += record.byteLength;
      if (this.receiveBytes + this.bodyBytes > QUEUE_LIMIT) { this.close(capacityError()); return; }
      this.receiving = this.receiving.then(async () => {
        try { await this.accept(JSON.parse(decoder.decode(await this.receiveCipher.open(record)))); await this.send({ op: "ack", sequence: this.acknowledged++ }); }
        finally { this.receiveBytes -= record.byteLength; }
      }).catch(() => this.close());
    };
    wire.onerror = () => this.close();
    wire.onclose = () => this.close();
  }

  static async connect(target: SecureTarget, purpose: SecurePurpose, signal?: AbortSignal): Promise<RemoteTransport> {
    signal?.throwIfAborted();
    const pending = await beginSecureHandshake(target, purpose);
    const url = new URL("/api/secure-channel", target.origin); url.protocol = "wss:";
    const wire = new WebSocket(url); wire.binaryType = "arraybuffer";
    let transport: RemoteTransport | undefined;
    let rejectHandshake: (error: Error) => void = () => {};
    const abort = () => { wire.close(); transport?.close(); rejectHandshake(connectionEnded()); };
    signal?.addEventListener("abort", abort, { once: true });
    const timer = setTimeout(abort, 10_000);
    try {
      const server = await new Promise<unknown>((resolve, reject) => {
        rejectHandshake = reject;
        wire.onopen = () => wire.send(JSON.stringify(pending.hello));
        wire.onerror = wire.onclose = () => reject(connectionEnded());
        wire.onmessage = event => {
          if (typeof event.data !== "string" || event.data.length > 4096) { reject(connectionEnded()); return; }
          wire.onmessage = () => reject(connectionEnded());
          try { resolve(JSON.parse(event.data)); } catch { reject(connectionEnded()); }
        };
      });
      const agreement = await pending.finish(server);
      if (wire.readyState !== WebSocket.OPEN) throw connectionEnded();
      signal?.throwIfAborted();
      transport = new RemoteTransport(wire, agreement);
      await new Promise<void>((resolve, reject) => {
        transport!.ready = { resolve, reject };
        void transport!.send({ op: "confirm" }).catch(reject);
      });
      signal?.throwIfAborted();
      return transport;
    } catch (error) { wire.close(); transport?.close(); throw error; }
    finally { clearTimeout(timer); signal?.removeEventListener("abort", abort); }
  }

  get active() { return !this.closed; }
  onClose(listener: () => void): () => void {
    if (this.closed) { listener(); return () => {}; }
    this.listeners.add(listener); return () => { this.listeners.delete(listener); };
  }
  close(error = connectionEnded()) {
    if (this.closed) return;
    this.closed = true; this.wire.close(); this.ready?.reject(error); this.ready = null;
    for (const [id, wait] of this.http) { wait.reject(error); wait.controller?.error(error); this.removeHttp(id); }
    for (const socket of this.sockets.values()) socket.ended();
    this.sockets.clear(); this.bodyBytes = 0;
    for (const listener of this.listeners) listener();
    this.listeners.clear();
  }
  private send(frame: object): Promise<void> {
    if (this.closed) return Promise.reject(connectionEnded());
    const bytes = encoder.encode(JSON.stringify(frame));
    if (bytes.length > 96 * 1024 || this.sendBytes + this.wire.bufferedAmount + bytes.length + 24 > QUEUE_LIMIT) {
      this.close(capacityError()); return Promise.reject(capacityError());
    }
    this.sendBytes += bytes.length + 24;
    const next = this.sending.then(async () => {
      if (this.closed) throw connectionEnded();
      const record = await this.sendCipher.seal(bytes);
      if (this.closed || this.wire.readyState !== WebSocket.OPEN) throw connectionEnded();
      this.wire.send(record);
    }).finally(() => { this.sendBytes -= bytes.length + 24; });
    this.sending = next.catch(() => this.close());
    return next;
  }
  private allocate() {
    if (this.closed) throw connectionEnded();
    if (this.http.size + this.sockets.size >= 32) throw capacityError();
    if (this.nextId > 0xffffffff) { this.close(); throw connectionEnded(); }
    return this.nextId++;
  }
  fetch(path: string, init: RequestInit = {}): Promise<Response> {
    if (!path.startsWith("/") || path.startsWith("//")) return Promise.reject(connectionEnded());
    init.signal?.throwIfAborted();
    const id = this.allocate();
    const request = new Request(new URL(path, this.hello.origin), { ...init, credentials: "omit", redirect: "error", referrerPolicy: "no-referrer" });
    const result = new Promise<Response>((resolve, reject) => {
      const abort = () => { this.cancelHttp(id, init.signal?.reason instanceof Error ? init.signal.reason : connectionEnded()); };
      init.signal?.addEventListener("abort", abort, { once: true });
      this.http.set(id, { resolve, reject, chunks: [], end: false, pulling: false, responded: false,
        removeAbort: () => init.signal?.removeEventListener("abort", abort) });
    });
    void (async () => {
      await this.send({ op: "request", id, method: request.method, path, headers: [...request.headers] });
      const reader = request.body?.getReader();
      try {
        if (reader) for (;;) {
          if (!this.http.has(id)) { await reader.cancel(); return; }
          const part = await reader.read(); if (part.done) break;
          for (let offset = 0; offset < part.value.byteLength; offset += REMOTE_CHUNK) {
            await this.send({ op: "data", id, data: encodeBase64Url(part.value.slice(offset, offset + REMOTE_CHUNK)), end: false });
          }
        }
        await this.send({ op: "data", id, data: "", end: true });
      } finally { reader?.releaseLock(); }
    })().catch(error => this.cancelHttp(id, error instanceof Error ? error : connectionEnded()));
    return result;
  }
  private removeHttp(id: number) {
    const wait = this.http.get(id); if (!wait) return;
    wait.removeAbort(); wait.releaseFrame?.(); wait.releaseFrame = undefined; for (const chunk of wait.chunks) this.bodyBytes -= chunk.length;
    wait.chunks = []; this.http.delete(id);
  }
  private cancelHttp(id: number, error: Error) {
    const wait = this.http.get(id); if (!wait) return;
    wait.reject(error); wait.controller?.error(error); this.removeHttp(id);
    void this.send({ op: "cancel", id }).catch(() => {});
  }
  private pull(id: number) {
    const wait = this.http.get(id); if (!wait?.controller) return;
    if (wait.pulling && wait.chunks.length) {
      const chunk = wait.chunks.shift()!; this.bodyBytes -= chunk.length;
      wait.pulling = false; wait.controller.enqueue(chunk); wait.releaseFrame?.(); wait.releaseFrame = undefined;
    }
    if (wait.end && !wait.chunks.length) { wait.controller.close(); this.removeHttp(id); }
  }
  private async accept(value: unknown) {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw connectionEnded();
    const frame = value as Record<string, unknown>;
    if (frame.op === "ready") { exact(frame, ["op"]); if (!this.ready) throw connectionEnded(); this.ready.resolve(); this.ready = null; return; }
    if (this.ready || !Number.isSafeInteger(frame.id) || Number(frame.id) < 1 || Number(frame.id) >= this.nextId) throw connectionEnded();
    const id = Number(frame.id);
    const wait = this.http.get(id), socket = this.sockets.get(id);
    if (!wait && !socket) return; // Terminal/cancelled child frames never revive it.
    if (frame.op === "error") {
      exact(frame, ["op", "id", "code"]);
      if (wait) this.cancelHttp(id, frame.code === "capacity" ? capacityError() : connectionEnded());
      if (socket) { socket.ended(); this.sockets.delete(id); }
    } else if (frame.op === "response" && wait) {
      exact(frame, ["op", "id", "status", "headers"]);
      if (wait.responded || !Number.isInteger(frame.status) || Number(frame.status) < 200 || Number(frame.status) > 599 || !Array.isArray(frame.headers)) throw connectionEnded();
      const headers = new Headers(frame.headers as [string, string][]); wait.responded = true;
      const stream = new ReadableStream<Uint8Array<ArrayBuffer>>({
        start: controller => { wait.controller = controller; },
        pull: () => { wait.pulling = true; this.pull(id); },
        cancel: () => this.cancelHttp(id, connectionEnded()),
      }, { highWaterMark: 0 });
      wait.resolve(new Response([204, 205, 304].includes(Number(frame.status)) ? null : stream, { status: Number(frame.status), headers }));
    } else if (frame.op === "data" && wait) {
      exact(frame, ["op", "id", "data", "end"]);
      if (!wait.responded || wait.end || typeof frame.data !== "string" || typeof frame.end !== "boolean") throw connectionEnded();
      const bytes = frame.data === "" ? new Uint8Array() : decodeCanonicalBase64Url(frame.data);
      if (!bytes || bytes.length > REMOTE_CHUNK) throw connectionEnded();
      this.bodyBytes += bytes.length;
      if (this.bodyBytes + this.receiveBytes > QUEUE_LIMIT) throw capacityError();
      const consumed = bytes.length ? new Promise<void>(resolve => { wait.releaseFrame = resolve; }) : Promise.resolve();
      if (bytes.length) wait.chunks.push(bytes); wait.end = frame.end; this.pull(id);
      await consumed;
    } else if (frame.op === "socket_open" && socket) { exact(frame, ["op", "id"]); socket.opened(); }
    else if (frame.op === "socket_data" && socket) {
      exact(frame, ["op", "id", "data", "end"]);
      if (typeof frame.data !== "string" || typeof frame.end !== "boolean") throw connectionEnded();
      const bytes = frame.data === "" ? new Uint8Array() : decodeCanonicalBase64Url(frame.data);
      if (!bytes || bytes.length > REMOTE_CHUNK) throw connectionEnded();
      this.bodyBytes += bytes.length;
      if (this.bodyBytes + this.receiveBytes > QUEUE_LIMIT) throw capacityError();
      this.bodyBytes -= socket.receive(bytes, frame.end);
    } else if (frame.op === "socket_close" && socket) { exact(frame, ["op", "id"]); this.bodyBytes -= socket.pendingBytes; socket.ended(); this.sockets.delete(id); }
    else throw connectionEnded();
  }
  openSocket(ticket: string): RoomSocketConnection {
    const id = this.allocate();
    const socket = new RemoteSocket(async text => {
      const bytes = encoder.encode(text); if (bytes.length > 256 * 1024) throw capacityError();
      for (let offset = 0; offset < bytes.length; offset += REMOTE_CHUNK) {
        await this.send({ op: "socket_data", id, data: encodeBase64Url(bytes.slice(offset, offset + REMOTE_CHUNK)), end: offset + REMOTE_CHUNK >= bytes.length });
      }
    }, () => { this.bodyBytes -= socket.pendingBytes; this.sockets.delete(id); void this.send({ op: "socket_close", id }).catch(() => {}); });
    this.sockets.set(id, socket);
    void this.send({ op: "socket_open", id, ticket }).catch(() => socket.ended());
    return socket;
  }
}

class RemoteSocket implements RoomSocketConnection {
  readyState = 0;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  private chunks: Uint8Array<ArrayBuffer>[] = [];
  pendingBytes = 0;
  private sender: (text: string) => Promise<void>;
  private closer: () => void;
  constructor(sender: (text: string) => Promise<void>, closer: () => void) { this.sender = sender; this.closer = closer; }
  opened() { if (this.readyState !== 0) throw connectionEnded(); this.readyState = 1; this.onopen?.(new Event("open")); }
  receive(bytes: Uint8Array<ArrayBuffer>, end: boolean): number {
    if (this.readyState !== 1 || this.pendingBytes + bytes.length > 256 * 1024) throw connectionEnded();
    this.chunks.push(bytes); this.pendingBytes += bytes.length;
    if (!end) return 0;
    const joined = new Uint8Array(this.pendingBytes); let offset = 0;
    for (const chunk of this.chunks) { joined.set(chunk, offset); offset += chunk.length; }
    this.chunks = []; this.pendingBytes = 0;
    this.onmessage?.(new MessageEvent("message", { data: decoder.decode(joined) })); return joined.length;
  }
  send(text: string) { if (this.readyState !== 1) throw connectionEnded(); void this.sender(text).catch(() => { this.onerror?.(new Event("error")); this.close(); }); }
  close() { if (this.readyState === 3) return; this.closer(); this.ended(); }
  ended() { if (this.readyState === 3) return; this.readyState = 3; this.chunks = []; this.pendingBytes = 0; this.onclose?.(new CloseEvent("close", { code: 1000 })); }
}
