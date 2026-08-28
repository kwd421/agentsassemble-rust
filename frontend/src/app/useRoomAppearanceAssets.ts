import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  fetchRoomAppearanceBlob,
  type RoomAppearanceReadAuthority,
} from "../api/roomAppearance";
import {
  completeRoomAppearance,
  type RoomAppearance,
} from "../lib/roomAppearance";
import { roomAppearanceAssetReference } from "../lib/roomAppearanceAsset";
import { roomSettingsKey, type RoomDockItem } from "../lib/roomDockModel";
import type { DesktopManagerRoomAuthority } from "../lib/desktopBridge";

type SettingsStatus = "loading" | "ready" | "saving" | "stale" | "error";

type UseRoomAppearanceAssetsOptions = {
  rooms: RoomDockItem[];
  activeRoomId: string;
  activeRemoteRoomId: string;
  remoteSessionToken: string;
  canonicalAppearanceFor: (room: RoomDockItem) => RoomAppearance;
  settingsStatusFor: (room: RoomDockItem) => SettingsStatus;
  resolveLocalManager: (roomDockId: string) => DesktopManagerRoomAuthority;
};

type DesiredAsset = {
  key: string;
  roomKey: string;
  canonicalUrl: string;
  authority: RoomAppearanceReadAuthority;
  authorityKey: string;
  pendingPreferred: boolean;
};

type AssetRequest = DesiredAsset & {
  controller: AbortController;
  mode: "pending" | "bound";
};

function assetKey(roomKey: string, canonicalUrl: string) {
  return `${roomKey}\0${canonicalUrl}`;
}

function localAuthorityKey(manager: DesktopManagerRoomAuthority) {
  return [
    "local",
    manager.server_id,
    manager.authority_lineage_id,
    manager.room_id,
    manager.room_uid,
  ].join(":");
}

function sameStrings(
  left: Record<string, string>,
  right: Record<string, string>
) {
  const leftKeys = Object.keys(left);
  const rightKeys = Object.keys(right);
  return (
    leftKeys.length === rightKeys.length &&
    leftKeys.every((key) => left[key] === right[key])
  );
}

function omitKeys(
  source: Record<string, string>,
  removed: ReadonlySet<string>
) {
  if (![...removed].some((key) => key in source)) return source;
  return Object.fromEntries(
    Object.entries(source).filter(([key]) => !removed.has(key))
  );
}

export function useRoomAppearanceAssets({
  rooms,
  activeRoomId,
  activeRemoteRoomId,
  remoteSessionToken,
  canonicalAppearanceFor,
  settingsStatusFor,
  resolveLocalManager,
}: UseRoomAppearanceAssetsOptions) {
  const [resolvedUrls, setResolvedUrls] = useState<Record<string, string>>({});
  const [requestErrors, setRequestErrors] = useState<Record<string, string>>({});
  const [staticErrors, setStaticErrors] = useState<Record<string, string>>({});
  const [retryRevision, setRetryRevision] = useState(0);
  const requestsRef = useRef(new Map<string, AssetRequest>());
  const liveObjectUrlsRef = useRef(new Set<string>());
  const renderedObjectUrlsRef = useRef(new Set<string>());
  const remoteCredentialRef = useRef({ value: remoteSessionToken, revision: 0 });
  if (remoteCredentialRef.current.value !== remoteSessionToken) {
    remoteCredentialRef.current = {
      value: remoteSessionToken,
      revision: remoteCredentialRef.current.revision + 1,
    };
  }
  const remoteCredentialRevision = remoteCredentialRef.current.revision;

  useEffect(() => {
    const desired = new Map<string, DesiredAsset>();
    const nextStaticErrors: Record<string, string> = {};

    for (const room of rooms) {
      const roomKey = roomSettingsKey(room);
      const appearance = canonicalAppearanceFor(room);
      const canonicalUrls = [
        appearance.iconImage,
        room.id === activeRoomId ? appearance.bannerImage : undefined,
      ].filter((value): value is string => Boolean(value));
      if (!canonicalUrls.length) continue;

      let authority: RoomAppearanceReadAuthority;
      let authorityKey: string;
      try {
        if (
          room.roomOrigin === "remote_server" &&
          room.id === activeRemoteRoomId &&
          remoteSessionToken
        ) {
          authority = { kind: "remote", sessionToken: remoteSessionToken };
          authorityKey = `remote:${roomKey}:${remoteCredentialRevision}`;
        } else if (room.roomOrigin !== "remote_server") {
          const manager = resolveLocalManager(room.id);
          authority = { kind: "local", manager };
          authorityKey = localAuthorityKey(manager);
        } else {
          throw new Error("현재 방 외형 조회 권위를 사용할 수 없습니다.");
        }
      } catch (error) {
        nextStaticErrors[roomKey] =
          error instanceof Error ? error.message : "방 외형 조회 권위를 확인할 수 없습니다.";
        continue;
      }

      for (const canonicalUrl of new Set(canonicalUrls)) {
        try {
          const reference = roomAppearanceAssetReference(canonicalUrl);
          const key = assetKey(roomKey, reference.url);
          desired.set(key, {
            key,
            roomKey,
            canonicalUrl: reference.url,
            authority,
            authorityKey,
            pendingPreferred:
              authority.kind === "local" && settingsStatusFor(room) === "saving",
          });
        } catch (error) {
          nextStaticErrors[roomKey] =
            error instanceof Error ? error.message : "방 외형 자산 참조가 올바르지 않습니다.";
        }
      }
    }

    setStaticErrors((current) =>
      sameStrings(current, nextStaticErrors) ? current : nextStaticErrors
    );

    const removed = new Set<string>();
    for (const [key, current] of requestsRef.current) {
      if (!desired.has(key)) {
        current.controller.abort();
        requestsRef.current.delete(key);
        removed.add(key);
      }
    }

    for (const [key, next] of desired) {
      const current = requestsRef.current.get(key);
      const sameAuthority = current?.authorityKey === next.authorityKey;
      const mode =
        next.authority.kind === "remote" || (sameAuthority && current?.mode === "bound")
          ? "bound"
          : next.pendingPreferred
            ? "pending"
            : "bound";
      if (sameAuthority && current?.mode === mode) continue;

      current?.controller.abort();
      if (current && !sameAuthority) removed.add(key);
      const request: AssetRequest = {
        ...next,
        mode,
        controller: new AbortController(),
      };
      requestsRef.current.set(key, request);
      setRequestErrors((errors) => omitKeys(errors, new Set([key])));

      void fetchRoomAppearanceBlob(
        request.canonicalUrl,
        request.authority,
        request.mode,
        request.controller.signal
      )
        .then((blob) => {
          if (
            request.controller.signal.aborted ||
            requestsRef.current.get(key) !== request
          ) {
            return;
          }
          const objectUrl = URL.createObjectURL(blob);
          liveObjectUrlsRef.current.add(objectUrl);
          setResolvedUrls((urls) => ({ ...urls, [key]: objectUrl }));
        })
        .catch((error) => {
          if (
            request.controller.signal.aborted ||
            requestsRef.current.get(key) !== request
          ) {
            return;
          }
          setResolvedUrls((urls) => omitKeys(urls, new Set([key])));
          setRequestErrors((errors) => ({
            ...errors,
            [key]:
              error instanceof Error ? error.message : "방 외형 자산을 읽을 수 없습니다.",
          }));
        });
    }

    if (removed.size) {
      setResolvedUrls((urls) => omitKeys(urls, removed));
      setRequestErrors((errors) => omitKeys(errors, removed));
    }
  }, [
    activeRemoteRoomId,
    activeRoomId,
    canonicalAppearanceFor,
    remoteCredentialRevision,
    remoteSessionToken,
    resolveLocalManager,
    retryRevision,
    rooms,
    settingsStatusFor,
  ]);

  useEffect(() => {
    const next = new Set(Object.values(resolvedUrls));
    for (const objectUrl of renderedObjectUrlsRef.current) {
      if (!next.has(objectUrl)) {
        URL.revokeObjectURL(objectUrl);
        liveObjectUrlsRef.current.delete(objectUrl);
      }
    }
    renderedObjectUrlsRef.current = next;
  }, [resolvedUrls]);

  useEffect(
    () => () => {
      for (const request of requestsRef.current.values()) {
        request.controller.abort();
      }
      requestsRef.current.clear();
      for (const objectUrl of liveObjectUrlsRef.current) {
        URL.revokeObjectURL(objectUrl);
      }
      liveObjectUrlsRef.current.clear();
    },
    []
  );

  const appearanceFor = useCallback(
    (room: RoomDockItem) => {
      const canonical = completeRoomAppearance(canonicalAppearanceFor(room));
      const roomKey = roomSettingsKey(room);
      return {
        ...canonical,
        bannerImage:
          room.id === activeRoomId && canonical.bannerImage
            ? resolvedUrls[assetKey(roomKey, canonical.bannerImage)]
            : undefined,
        iconImage: canonical.iconImage
          ? resolvedUrls[assetKey(roomKey, canonical.iconImage)]
          : undefined,
      };
    },
    [activeRoomId, canonicalAppearanceFor, resolvedUrls]
  );

  const appearances = useMemo(
    () =>
      Object.fromEntries(
        rooms.map((room) => [roomSettingsKey(room), appearanceFor(room)])
      ),
    [appearanceFor, rooms]
  );

  const errorFor = useCallback(
    (room: RoomDockItem) => {
      const roomKey = roomSettingsKey(room);
      if (staticErrors[roomKey]) return staticErrors[roomKey];
      for (const [key, message] of Object.entries(requestErrors)) {
        if (requestsRef.current.get(key)?.roomKey === roomKey) return message;
      }
      return "";
    },
    [requestErrors, staticErrors]
  );

  const retry = useCallback((room: RoomDockItem) => {
    const roomKey = roomSettingsKey(room);
    for (const [key, request] of requestsRef.current) {
      if (request.roomKey === roomKey) {
        request.controller.abort();
        requestsRef.current.delete(key);
      }
    }
    setRetryRevision((revision) => revision + 1);
  }, []);

  return { appearances, appearanceFor, errorFor, retry };
}
