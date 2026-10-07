import { useEffect, useState, type ImgHTMLAttributes } from "react";
import { remoteWorkspaceSnapshot } from "../../lib/remote/remoteWorkspace";
import { createResourceUrl, releaseResourceBlob, readResourceBlob, remoteResourcePath, revokeResourceUrl } from "../../lib/remote/remoteResources";

export function useResourceImage(source: string | undefined) {
  const [loaded, setLoaded] = useState<{ source: string; url: string; error?: string }>();
  let path: string | null = null;
  let invalid = false;
  try { path = remoteResourcePath(source); } catch { invalid = true; }
  useEffect(() => {
    if (!path || !source) return;
    const transport = remoteWorkspaceSnapshot()!.transport;
    const controller = new AbortController(); let url = "";
    void transport.fetch(path, { signal: controller.signal }).then(async response => {
      if (!response.ok) throw new Error("사진을 불러오지 못했어요.");
      const blob = await readResourceBlob(response);
      try {
      if (!/^image\/(png|jpeg|webp)$/.test(blob.type)) throw new Error("사진 형식을 확인하지 못했어요.");
      controller.signal.throwIfAborted(); url = createResourceUrl(blob); setLoaded({ source, url });
      } catch (error) { releaseResourceBlob(blob); throw error; }
    }).catch(error => { if (!controller.signal.aborted) setLoaded({ source, url: "", error: error instanceof Error ? error.message : "사진을 불러오지 못했어요." }); });
    return () => { controller.abort(); if (url) revokeResourceUrl(url); };
  }, [source, path]);
  return { url: invalid ? undefined : path ? loaded?.source === source ? loaded?.url || undefined : undefined : source,
    error: invalid ? "사진 주소를 확인하지 못했어요." : loaded?.source === source ? loaded?.error : undefined };
}

export default function ResourceImage({ src, alt, ...props }: ImgHTMLAttributes<HTMLImageElement>) {
  const resource = useResourceImage(src);
  return <img {...props} src={resource.url} alt={resource.error || alt} title={resource.error || props.title} />;
}
