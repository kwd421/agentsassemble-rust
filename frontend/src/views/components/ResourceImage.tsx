import { useEffect, useState, type ImgHTMLAttributes } from "react";
import { hasRemoteWorkspace, remoteTransportForOrigin } from "../../lib/remote/remoteWorkspace";
import { createResourceUrl, releaseResourceBlob, readResourceBlob, remoteResourcePath, revokeResourceUrl } from "../../lib/remote/remoteResources";
import type { RemoteTransport } from "../../lib/remote/remoteTransport";

function originOf(source: string): string | null {
  try {
    const url = new URL(source);
    if (url.protocol === "http:" || url.protocol === "https:") return url.origin;
  } catch { /* relative reference: caller passes origin */ }
  return null;
}

export function useResourceImage(source: string | undefined, origin?: string | null) {
  const [loaded, setLoaded] = useState<{ source: string; url: string; error?: string }>();
  // Room-relative "/api/..." references require the room's own origin
  // explicitly so one host's image is never fetched through another host's
  // channel. Absolute references carry their own origin but are only fetched
  // encrypted when it belongs to a live host; anything else (bundled assets,
  // central icons, foreign avatars) renders directly like before.
  let path: string | null = null;
  let invalid = false;
  let transport: RemoteTransport | null = null;
  try {
    if (source && !/^(blob:|data:)/.test(source)) {
      const candidate = origin ?? originOf(source);
      if (candidate && remoteTransportForOrigin(candidate)) {
        path = remoteResourcePath(source, candidate);
        transport = remoteTransportForOrigin(candidate);
      } else if (!candidate && source.startsWith("/api/") && hasRemoteWorkspace()) {
        throw new Error("서버를 확인하지 못했어요. 방의 서버에서 다시 시도해 주세요.");
      }
    }
  } catch { invalid = true; }
  useEffect(() => {
    if (!path || !source || !transport) return;
    const active = transport;
    const controller = new AbortController(); let url = "";
    void active.fetch(path, { signal: controller.signal }).then(async response => {
      if (!response.ok) throw new Error("사진을 불러오지 못했어요.");
      const blob = await readResourceBlob(response, 10 * 1024 * 1024, active);
      try {
      if (!/^image\/(png|jpeg|webp)$/.test(blob.type)) throw new Error("사진 형식을 확인하지 못했어요.");
      controller.signal.throwIfAborted(); url = createResourceUrl(blob); setLoaded({ source, url });
      } catch (error) { releaseResourceBlob(blob); throw error; }
    }).catch(error => { if (!controller.signal.aborted) setLoaded({ source, url: "", error: error instanceof Error ? error.message : "사진을 불러오지 못했어요." }); });
    return () => { controller.abort(); if (url) revokeResourceUrl(url); };
    // `path`/`transport` are derived from `source`/`origin` above.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, path]);
  return { url: invalid ? undefined : path ? loaded?.source === source ? loaded?.url || undefined : undefined : source,
    error: invalid ? "사진 주소를 확인하지 못했어요." : loaded?.source === source ? loaded?.error : undefined };
}

export default function ResourceImage({ src, origin, alt, ...props }: ImgHTMLAttributes<HTMLImageElement> & { origin?: string | null }) {
  const resource = useResourceImage(src, origin);
  return <img {...props} src={resource.url} alt={resource.error || alt} title={resource.error || props.title} />;
}
