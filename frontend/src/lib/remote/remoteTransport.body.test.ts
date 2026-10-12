// @vitest-environment node
import { afterEach, expect, test, vi } from "vitest";
import { RemoteTransport } from "./remoteTransport";
import { decodeCanonicalBase64Url, encodeBase64Url } from "../base64Url";
// The shared setup registers dialog hooks even in this isolated HTTP-only worker.
vi.hoisted(() => Object.defineProperty(globalThis, "HTMLDialogElement", { configurable: true, value: class {} }));
const encoder = new TextEncoder(), decoder = new TextDecoder();
vi.mock("./secureCrypto", () => ({
  beginSecureHandshake: async () => ({ hello: {}, finish: async () => ({
    hello: { origin: "https://host.test" },
    send: { seal: async (bytes: Uint8Array) => bytes.slice().buffer },
    receive: { open: async (record: ArrayBuffer) => new Uint8Array(record) },
  }) }),
}));
// Public connect/fetch boundary, with a protocol peer that echoes received bytes.
class Peer {
  static OPEN = 1;
  static latest: Peer;
  static autoConsume = true;
  static echoBody = true;
  pendingCredits: number[] = [];
  receivedChunks = 0;
  onData?: () => void;
  readyState = 1; bufferedAmount = 0;
  onopen?: () => void; onmessage?: (event: { data: string | ArrayBuffer }) => void;
  onerror?: () => void; onclose?: () => void;
  requests = new Map<number, { headers: [string, string][]; chunks: Uint8Array[] }>();
  constructor() { Peer.latest = this; queueMicrotask(() => this.onopen?.()); }
  consume() { const id = this.pendingCredits.shift(); if (id !== undefined) this.emit({ op: "consumed", id }); }
  emit(frame: object) { queueMicrotask(() => this.onmessage?.({ data: encoder.encode(JSON.stringify(frame)).buffer })); }
  send(record: string | ArrayBuffer) {
    if (typeof record === "string") { queueMicrotask(() => this.onmessage?.({ data: "{}" })); return; }
    const frame = JSON.parse(decoder.decode(record));
    if (frame.op === "confirm") this.emit({ op: "ready" });
    if (frame.op === "request") this.requests.set(frame.id, { headers: frame.headers, chunks: [] });
    if (frame.op !== "data") return;
    const request = this.requests.get(frame.id)!;
    if (frame.data) {
      const bytes = decodeCanonicalBase64Url(frame.data)!;
      if (bytes.length > 64 * 1024) throw new Error("peer request chunk capacity exceeded");
      request.chunks.push(bytes);
    }
    if (!frame.end) {
      this.receivedChunks++; this.pendingCredits.push(frame.id);
      if (Peer.autoConsume) this.consume();
      this.onData?.(); return;
    }
    let body = new Uint8Array(request.chunks.reduce((length, part) => length + part.length, 0));
    let offset = 0; for (const part of request.chunks) { body.set(part, offset); offset += part.length; }
    if (!Peer.echoBody) body = encoder.encode(JSON.stringify({ received: body.length }));
    this.emit({ op: "response", id: frame.id, status: 200, headers: request.headers });
    for (let start = 0; start < body.length; start += 64 * 1024)
      this.emit({ op: "data", id: frame.id, data: encodeBase64Url(body.slice(start, start + 64 * 1024)), end: false });
    this.emit({ op: "data", id: frame.id, data: "", end: true });
  }
  close() { this.readyState = 3; }
}
const transports: RemoteTransport[] = [];
afterEach(() => { for (const transport of transports.splice(0)) transport.close(); vi.unstubAllGlobals(); Peer.autoConsume = true; Peer.echoBody = true; });
async function connectWithoutRequestBody() {
  const BrowserRequest = Request;
  vi.stubGlobal("Request", class extends BrowserRequest {
    constructor(input: RequestInfo | URL, init?: RequestInit) {
      super(input, init); Object.defineProperty(this, "body", { value: undefined });
    }
  });
  vi.stubGlobal("WebSocket", Peer);
  const transport = await RemoteTransport.connect({ origin: "https://host.test" } as never, "member_admission");
  transports.push(transport); return transport;
}
// Old optional Request.body reader sends an empty body: JSON parsing, multipart
// decoding and the stream byte oracle fail at the returned HTTP boundary.
test("browser without Request.body sends member JSON over the encrypted HTTP protocol", async () => {
  const transport = await connectWithoutRequestBody();
  const body = { invite_token: "fixture-invite" };
  const response = await transport.fetch("/api/room-invite/member-challenge", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
  });
  expect(await response.json()).toEqual(body);
});
test("multipart header boundary decodes the original field and file bytes", async () => {
  const transport = await connectWithoutRequestBody();
  const form = new FormData(); form.set("label", "프로필"); form.set("file", new Blob(["fixture bytes"]), "profile.txt");
  const decoded = await (await transport.fetch("/api/uploads", { method: "POST", body: form })).formData();
  expect(decoded.get("label")).toBe("프로필");
  expect(await (decoded.get("file") as File).text()).toBe("fixture bytes");
});
test("streamed request crosses multiple bounded wire chunks without losing bytes", async () => {
  const transport = await connectWithoutRequestBody();
  const payload = new Uint8Array(3 * 64 * 1024 + 19).fill(79);
  const stream = new ReadableStream({ start(controller) { controller.enqueue(payload); controller.close(); } });
  const response = await transport.fetch("/api/uploads", { method: "POST", body: stream, duplex: "half" } as RequestInit);
  expect(new Uint8Array(await response.arrayBuffer())).toEqual(payload);
});

// A turn barrier drains encryption/enqueue microtasks without an arbitrary sleep.
const drained = () => new Promise<void>(resolve => {
  const channel = new MessageChannel();
  channel.port1.onmessage = () => { channel.port1.close(); channel.port2.close(); resolve(); };
  channel.port2.postMessage(null);
});
test("10 MiB uploads wait for receiver credit, including across concurrent requests", async () => {
  Peer.autoConsume = false; Peer.echoBody = false;
  const transport = await connectWithoutRequestBody(), peer = Peer.latest;
  const payload = new Uint8Array(10 * 1024 * 1024).fill(79);
  const firstData = new Promise<void>(resolve => { peer.onData = resolve; });
  const first = transport.fetch("/api/uploads", { method: "POST", body: payload });
  const second = transport.fetch("/api/uploads", { method: "POST", body: payload });
  await firstData; await drained();
  expect(peer.receivedChunks).toBe(1);
  expect(peer.pendingCredits).toHaveLength(1);
  Peer.autoConsume = true; peer.consume();
  const results = await Promise.all([first, second].map(async response => (await response).json()));
  expect(results).toEqual([{ received: payload.length }, { received: payload.length }]);
  expect(transport.active).toBe(true);
});

test.each(["cancel", "early response"])("%s releases only its upload credit and reader", async reason => {
  Peer.autoConsume = false;
  const transport = await connectWithoutRequestBody(), peer = Peer.latest;
  const firstData = new Promise<void>(resolve => { peer.onData = resolve; });
  const abort = new AbortController(); let cancelled = false;
  const body = new ReadableStream({
    start(controller) { controller.enqueue(new Uint8Array(64 * 1024)); },
    cancel() { cancelled = true; },
  });
  const first = transport.fetch("/api/uploads", { method: "POST", body, signal: abort.signal, duplex: "half" } as RequestInit);
  const outcome = first.then(response => response.status, () => "cancelled");
  await firstData;
  const id = peer.pendingCredits.shift()!;
  Peer.autoConsume = true;
  const second = transport.fetch("/api/uploads", { method: "POST", body: "second" });
  if (reason === "cancel") abort.abort();
  else {
    peer.emit({ op: "response", id, status: 408, headers: [] });
    peer.emit({ op: "data", id, data: "", end: true });
  }
  expect(await outcome).toBe(reason === "cancel" ? "cancelled" : 408);
  expect(await (await second).text()).toBe("second");
  expect(cancelled).toBe(true); expect(transport.active).toBe(true);
});
