import { hasRemoteWorkspace } from "./remoteWorkspace";
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

export function createResourceUrl(blob: Blob, transport?: RemoteTransport | null): string {
  const reserved = blobs.get(blob);
  // A blob already bound by readResourceBlob carries its own host budget, so
  // it never needs (or trusts) an implicit workspace pick.
  if (reserved) {
    if (transport && transport !== undefined && reserved !== budgetFor(transport)) { releaseResourceBlob(blob); throw new Error("서버 연결이 바뀌었어요."); }
    const budget = reserved;
    if (!transport && hasRemoteWorkspace()) {
      // Bound blobs are already charged; creating the object URL needs no host.
    }
    const url = URL.createObjectURL(blob);
    reserved.blobs.delete(blob); blobs.delete(blob);
    budget.urls.add(url); urls.set(url, { budget, size: blob.size });
    return url;
  }
  const host = transport ?? null;
  if (!host) {
    // Local files (composer previews, cropped photos) never touch a host
    // budget; host-fetched blobs already carry their own bound budget above.
    releaseResourceBlob(blob); return URL.createObjectURL(blob);
  }
  if (!host.active) { releaseResourceBlob(blob); throw new Error("서버 연결이 끝났어요."); }
  const budget = budgetFor(host);
  if (budget.bytes + blob.size > LIMIT) throw capacity();
  const url = URL.createObjectURL(blob);
  budget.bytes += blob.size;
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
// separately and released by the existing component/download lifecycle. The
// caller passes the room's own transport; with any host live an omitted host
// fails closed instead of charging another host's budget.
export async function readResourceBlob(response: Response, maximum = 10 * 1024 * 1024, transport?: RemoteTransport | null): Promise<Blob> {
  // transport === null is an explicit local/plain read (no host budget).
  // transport === undefined with a host live fails closed: the caller must
  // resolve the room's own transport instead of inheriting another host's.
  if (transport === null) return response.blob();
  const host = transport ?? null;
  if (!host) {
    if (hasRemoteWorkspace()) throw new Error("서버를 확인하지 못했어요. 방의 서버에서 다시 시도해 주세요.");
    return response.blob();
  }
  const budget = budgetFor(host);
  const reader = response.body?.getReader();
  if (!reader) return new Blob();
  const chunks: Uint8Array<ArrayBuffer>[] = []; let size = 0; let retained = false;
  try {
    for (;;) {
      const part = await reader.read(); if (part.done) break;
      if (size + part.value.length > maximum || budget.bytes + part.value.length > LIMIT) throw capacity();
      size += part.value.length; budget.bytes += part.value.length; chunks.push(part.value);
    }
    if (!host.active) throw new Error("서버 연결이 끝났어요.");
    const blob = new Blob(chunks, { type: response.headers.get("Content-Type") || "" });
    blobs.set(blob, budget); budget.blobs.add(blob); retained = true; return blob;
  } finally { if (!retained) budget.bytes -= size; await reader.cancel().catch(() => {}); reader.releaseLock(); }
}

export function remoteResourcePath(source: string | undefined, origin?: string | null): string | null {
  if (!source || /^(blob:|data:)/.test(source)) return null;
  // The room's own host origin must be passed explicitly for host-relative
  // paths; with several hosts live a bare "/api/..." cannot pick its owner
  // implicitly. Bundled or page-relative assets keep the legacy direct path.
  const hostOrigin = origin ?? null;
  if (!hostOrigin) {
    if (source.startsWith("/api/") && hasRemoteWorkspace()) {
      throw new Error("서버를 확인하지 못했어요. 방의 서버에서 다시 시도해 주세요.");
    }
    return null;
  }
  const url = new URL(source, source.startsWith("/api/") ? hostOrigin : window.location.href);
  if (url.origin !== hostOrigin) return null;
  if (!/^\/api\/(attachments|agent-avatars)\//.test(url.pathname)) throw new Error("이 사진 주소를 사용할 수 없어요.");
  return `${url.pathname}${url.search}`;
}
