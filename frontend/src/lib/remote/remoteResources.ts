import { remoteWorkspaceSnapshot } from "./remoteWorkspace";
import type { RemoteTransport } from "./remoteTransport";

const LIMIT = 64 * 1024 * 1024;
type Budget = { bytes: number; urls: Set<string>; blobs: Set<Blob> };
const blobs = new WeakMap<Blob, Budget>();
const budgets = new WeakMap<RemoteTransport, Budget>();
const urls = new Map<string, { budget: Budget; size: number }>();
const capacity = () => new Error("사진과 첨부 파일이 많아요. 열려 있는 항목을 닫고 다시 시도해 주세요.");
function budgetFor(transport: RemoteTransport) {
  let budget = budgets.get(transport);
  if (!budget) {
    budget = { bytes: 0, urls: new Set(), blobs: new Set() }; budgets.set(transport, budget);
    transport.onClose(() => { for (const url of [...budget!.urls]) revokeResourceUrl(url); for (const blob of [...budget!.blobs]) releaseResourceBlob(blob); });
  }
  return budget;
}

export function createResourceUrl(blob: Blob): string {
  const reserved = blobs.get(blob);
  const transport = remoteWorkspaceSnapshot()?.transport;
  if (!transport) { releaseResourceBlob(blob); return URL.createObjectURL(blob); }
  if (!transport.active) { releaseResourceBlob(blob); throw new Error("서버 연결이 끝났어요."); }
  const budget = budgetFor(transport);
  if (reserved && reserved !== budget) { releaseResourceBlob(blob); throw new Error("서버 연결이 바뀌었어요."); }
  if (!reserved && budget.bytes + blob.size > LIMIT) throw capacity();
  const url = URL.createObjectURL(blob);
  if (reserved) { reserved.blobs.delete(blob); blobs.delete(blob); } else budget.bytes += blob.size;
  budget.urls.add(url); urls.set(url, { budget, size: blob.size });
  return url;
}
export function releaseResourceBlob(blob: Blob) {
  const budget = blobs.get(blob);
  if (budget) { budget.bytes -= blob.size; budget.blobs.delete(blob); blobs.delete(blob); }
}
export function revokeResourceUrl(url: string) {
  const entry = urls.get(url);
  if (entry) { entry.budget.bytes -= entry.size; entry.budget.urls.delete(url); urls.delete(url); }
  URL.revokeObjectURL(url);
}

// Bound response assembly before allocating a Blob. URL ownership is charged
// separately and released by the existing component/download lifecycle.
export async function readResourceBlob(response: Response, maximum = 10 * 1024 * 1024): Promise<Blob> {
  const transport = remoteWorkspaceSnapshot()?.transport;
  if (!transport) return response.blob();
  const budget = budgetFor(transport);
  const reader = response.body?.getReader();
  if (!reader) return new Blob();
  const chunks: Uint8Array<ArrayBuffer>[] = []; let size = 0; let retained = false;
  try {
    for (;;) {
      const part = await reader.read(); if (part.done) break;
      if (size + part.value.length > maximum || budget.bytes + part.value.length > LIMIT) throw capacity();
      size += part.value.length; budget.bytes += part.value.length; chunks.push(part.value);
    }
    if (!transport.active) throw new Error("서버 연결이 끝났어요.");
    const blob = new Blob(chunks, { type: response.headers.get("Content-Type") || "" });
    blobs.set(blob, budget); budget.blobs.add(blob); retained = true; return blob;
  } finally { if (!retained) budget.bytes -= size; await reader.cancel().catch(() => {}); reader.releaseLock(); }
}

export function remoteResourcePath(source: string | undefined): string | null {
  const workspace = remoteWorkspaceSnapshot();
  if (!workspace || !source || /^(blob:|data:)/.test(source)) return null;
  const url = new URL(source, source.startsWith("/api/") ? workspace.transport.hello.origin : window.location.href);
  if (url.origin !== workspace.transport.hello.origin) return null;
  if (!/^\/api\/(attachments|agent-avatars)\//.test(url.pathname)) throw new Error("이 사진 주소를 사용할 수 없어요.");
  return `${url.pathname}${url.search}`;
}
