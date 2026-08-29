import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { fetchPersonaThumbnail } from "../../api/personas";

type ThumbnailRequest = {
  controller: AbortController;
};

export function usePersonaThumbnails(
  personaIds: readonly string[],
  importGeneration: number
) {
  const desiredKey = [...new Set(personaIds)].sort().join("\0");
  const activeRef = useRef(true);
  const importGenerationRef = useRef(importGeneration);
  const requestsRef = useRef(new Map<string, ThumbnailRequest>());
  const liveObjectUrlsRef = useRef(new Set<string>());
  const renderedObjectUrlsRef = useRef(new Set<string>());
  const [urls, setUrls] = useState<Record<string, string>>({});
  const [failedIds, setFailedIds] = useState<ReadonlySet<string>>(new Set());

  useEffect(() => {
    const desired = new Set(desiredKey ? desiredKey.split("\0") : []);
    const removed = new Set<string>();
    if (importGenerationRef.current !== importGeneration) {
      importGenerationRef.current = importGeneration;
      for (const [personaId, request] of requestsRef.current) {
        request.controller.abort();
        removed.add(personaId);
      }
      requestsRef.current.clear();
    }
    for (const [personaId, request] of requestsRef.current) {
      if (desired.has(personaId)) continue;
      request.controller.abort();
      requestsRef.current.delete(personaId);
      removed.add(personaId);
    }
    if (removed.size) {
      setUrls((current) => omit(current, removed));
      setFailedIds((current) => difference(current, removed));
    }

    for (const personaId of desired) {
      if (requestsRef.current.has(personaId)) continue;
      const request: ThumbnailRequest = { controller: new AbortController() };
      requestsRef.current.set(personaId, request);
      void fetchPersonaThumbnail(personaId, request.controller.signal)
        .then((blob) => {
          if (
            !activeRef.current ||
            request.controller.signal.aborted ||
            requestsRef.current.get(personaId) !== request
          ) {
            return;
          }
          const objectUrl = URL.createObjectURL(blob);
          liveObjectUrlsRef.current.add(objectUrl);
          setUrls((current) => ({ ...current, [personaId]: objectUrl }));
          setFailedIds((current) => difference(current, new Set([personaId])));
        })
        .catch(() => {
          if (requestsRef.current.get(personaId) === request) {
            requestsRef.current.delete(personaId);
            if (!request.controller.signal.aborted && activeRef.current) {
              setFailedIds((current) => new Set(current).add(personaId));
            }
          }
        });
    }
  }, [desiredKey, importGeneration]);

  useLayoutEffect(() => {
    const next = new Set(Object.values(urls));
    for (const objectUrl of renderedObjectUrlsRef.current) {
      if (!next.has(objectUrl)) {
        URL.revokeObjectURL(objectUrl);
        liveObjectUrlsRef.current.delete(objectUrl);
      }
    }
    renderedObjectUrlsRef.current = next;
  }, [urls]);

  useLayoutEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
      for (const request of requestsRef.current.values()) {
        request.controller.abort();
      }
      requestsRef.current.clear();
      for (const objectUrl of liveObjectUrlsRef.current) {
        URL.revokeObjectURL(objectUrl);
      }
      liveObjectUrlsRef.current.clear();
      renderedObjectUrlsRef.current.clear();
    };
  }, []);

  return { urls, failedIds };
}

function omit(current: Record<string, string>, removed: ReadonlySet<string>) {
  return Object.fromEntries(
    Object.entries(current).filter(([personaId]) => !removed.has(personaId))
  );
}

function difference(current: ReadonlySet<string>, removed: ReadonlySet<string>) {
  return new Set([...current].filter((personaId) => !removed.has(personaId)));
}
