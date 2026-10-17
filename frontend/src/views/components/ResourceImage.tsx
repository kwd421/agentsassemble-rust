import { useEffect, useState, type ImgHTMLAttributes } from "react";
import { hasRemoteWorkspace, isKnownHostOrigin, remoteTransportForOrigin } from "../../lib/remote/remoteWorkspace";
import { createResourceUrl, releaseResourceBlob, readResourceBlob, remoteResourcePath, revokeResourceUrl } from "../../lib/remote/remoteResources";
import type { RemoteTransport } from "../../lib/remote/remoteTransport";
import { userFacingError } from "../../lib/apiErrors";

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
  let hostOriginWithoutTransport = false;
  try {
    if (source && !/^(blob:|data:)/.test(source)) {
      const candidate = origin ?? originOf(source);
      const live = candidate ? remoteTransportForOrigin(candidate) : null;
      if (candidate && live) {
        path = remoteResourcePath(source, candidate);
        transport = live;
      } else if (candidate && isKnownHostOrigin(candidate)) {
        // A host-origin image with no live transport (host offline,
        // republished origin, torn-down channel) renders nothing — never a
        // direct tunnel URL, which desktop CSP blocks and which carries no
        // auth. The fetch retries when a transport for the origin is live
        // again (see the effect deps below).
        hostOriginWithoutTransport = true;
      } else if (!candidate && source.startsWith("/api/") && hasRemoteWorkspace()) {
        throw new Error("서버를 확인하지 못했어요. 방의 서버에서 다시 시도해 주세요.");
      }
    }
  } catch { invalid = true; }
  useEffect(() => {
    if (!path || !source || !transport) return;
    const active = transport;
    // A reconnect lands on a new transport: drop the previous transport's
    // blob (its budget owner revoked it on close) instead of flashing it.
    setLoaded((current) => (current?.source === source ? undefined : current));
    const controller = new AbortController(); let url = "";
    void active.fetch(path, { signal: controller.signal }).then(async response => {
      if (!response.ok) throw new Error("사진을 불러오지 못했어요.");
      const blob = await readResourceBlob(response, 10 * 1024 * 1024, active);
      try {
      if (!/^image\/(png|jpeg|webp)$/.test(blob.type)) throw new Error("사진 형식을 확인하지 못했어요.");
      controller.signal.throwIfAborted(); url = createResourceUrl(blob); setLoaded({ source, url });
      } catch (error) { releaseResourceBlob(blob); throw error; }
    }).catch(error => { if (!controller.signal.aborted) setLoaded({ source, url: "", error: userFacingError(error, "사진을 불러오지 못했어요.") }); });
    return () => { controller.abort(); if (url) revokeResourceUrl(url); };
    // `path`/`transport` are derived from `source`/`origin` above; the
    // transport is a dep so a reconnected host refetches instead of
    // keeping the offline nothing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, path, transport]);
  if (hostOriginWithoutTransport && !invalid) return { url: undefined, error: undefined };
  return { url: invalid ? undefined : path ? loaded?.source === source ? loaded?.url || undefined : undefined : source,
    error: invalid ? "사진 주소를 확인하지 못했어요." : loaded?.source === source ? loaded?.error : undefined };
}

export default function ResourceImage({ src, origin, alt, ...props }: ImgHTMLAttributes<HTMLImageElement> & { origin?: string | null }) {
  const resource = useResourceImage(src, origin);
  return <img {...props} src={resource.url} alt={resource.error || alt} title={resource.error || props.title} />;
}
