import { useCallback, useEffect, useRef, useState } from "react";
import { fetchRooms } from "../api";
import {
  mergeServerRoomsIntoDock,
  persistableRoom,
  roomDockIdentity,
  type RoomDockItem,
} from "../lib/roomDockModel";
import {
  persistRoomDockItems,
  syncNativeRoomDockItems,
} from "../lib/roomDockPersistence";
import {
  bindRoomDirectoryAuthority,
  currentRoomDirectoryAuthority,
  retainRoomDirectoryAuthority,
  type RoomDirectoryAuthority,
  type TrustedServerProductSurface,
} from "../lib/roomDirectoryContract";
import {
  isDesktopWebview,
  parseDesktopManagerRoomAuthority,
  requestDesktopBootstrapStatus,
  type DesktopManagerRoomAuthority,
} from "../lib/desktopBridge";

type UseRoomDirectoryOptions = {
  initialRooms: RoomDockItem[];
  hostEnabled: boolean;
};

type RoomDirectorySyncIssue = {
  category: "room_directory_unconfirmed" | "room_directory_unavailable";
  message: string;
};

export function useRoomDirectory({
  initialRooms,
  hostEnabled,
}: UseRoomDirectoryOptions) {
  const initialSyncIssue: RoomDirectorySyncIssue | null = hostEnabled
    ? {
        category: "room_directory_unconfirmed",
        message: "Room directory is waiting for server confirmation.",
      }
    : null;
  const roomsRef = useRef<RoomDockItem[]>(initialRooms);
  const [rooms, setRooms] = useState<RoomDockItem[]>(initialRooms);
  const hostEnabledRef = useRef(hostEnabled);
  hostEnabledRef.current = hostEnabled;
  const syncIssueRef = useRef<RoomDirectorySyncIssue | null>(initialSyncIssue);
  const [syncIssue, setSyncIssueState] =
    useState<RoomDirectorySyncIssue | null>(initialSyncIssue);
  const membershipRevisionRef = useRef(0);
  const metadataRevisionRef = useRef(0);
  const hydrationEpochRef = useRef(0);
  const authorityRef = useRef<RoomDirectoryAuthority | null>(
    currentRoomDirectoryAuthority()
  );

  const publishSyncIssue = useCallback((issue: RoomDirectorySyncIssue | null) => {
    syncIssueRef.current = issue;
    setSyncIssueState(issue);
  }, []);

  const commit = useCallback((update: (current: RoomDockItem[]) => RoomDockItem[]) => {
    const next = update(roomsRef.current);
    roomsRef.current = next;
    setRooms(next);
    return next;
  }, []);

  const replaceRooms = useCallback(
    (nextRooms: RoomDockItem[]) => {
      membershipRevisionRef.current += 1;
      commit(() => [...nextRooms]);
    },
    [commit]
  );

  const prependRoom = useCallback(
    (room: RoomDockItem) => {
      membershipRevisionRef.current += 1;
      commit((current) => [room, ...current]);
    },
    [commit]
  );

  const mergeFlowRoom = useCallback(
    (roomOrNull: RoomDockItem | null) => {
      if (!roomOrNull) return;
      commit((current) => {
        const existingIndex = current.findIndex(
          (room) => roomDockIdentity(room) === roomDockIdentity(roomOrNull)
        );
        if (existingIndex >= 0) {
          const next = [...current];
          next[existingIndex] = {
            ...next[existingIndex],
            label: next[existingIndex].label || roomOrNull.label,
            topic: roomOrNull.topic,
          };
          return next;
        }
        membershipRevisionRef.current += 1;
        const [firstRoom, ...restRooms] = current;
        return firstRoom ? [firstRoom, roomOrNull, ...restRooms] : [roomOrNull];
      });
    },
    [commit]
  );

  const markRoomRead = useCallback(
    (roomId: string, readAt = new Date().toISOString()) => {
      commit((current) =>
        current.map((room) => (room.id === roomId ? { ...room, createdAt: readAt } : room))
      );
    },
    [commit]
  );

  const removeRoom = useCallback(
    (roomId: string) => {
      membershipRevisionRef.current += 1;
      return commit((current) => current.filter((room) => room.id !== roomId));
    },
    [commit]
  );

  const updateRoom = useCallback(
    (roomId: string, updates: Partial<RoomDockItem>) => {
      metadataRevisionRef.current += 1;
      commit((current) =>
        current.map((room) => (room.id === roomId ? { ...room, ...updates } : room))
      );
    },
    [commit]
  );

  const updateRoomByMeetingId = useCallback(
    (meetingId: string, updates: Partial<RoomDockItem>) => {
      metadataRevisionRef.current += 1;
      commit((current) =>
        current.map((room) => (room.meetingId === meetingId ? { ...room, ...updates } : room))
      );
    },
    [commit]
  );

  const resolveTrustedRoomDirectoryAuthority = useCallback(
    async (
      actual: RoomDirectoryAuthority
    ): Promise<TrustedServerProductSurface | null> => {
      if (isDesktopWebview()) {
        const bootstrap = await requestDesktopBootstrapStatus();
        if (bootstrap.phase !== "complete") {
          throw new Error("완료된 데스크톱 bootstrap 권위가 없습니다.");
        }
        authorityRef.current = retainRoomDirectoryAuthority(
          actual,
          authorityRef.current,
          bootstrap
        );
        return {
          revision: bootstrap.server_product_surface_revision,
          digest: bootstrap.server_product_surface_digest,
        };
      }
      authorityRef.current = retainRoomDirectoryAuthority(actual, authorityRef.current);
      return null;
    },
    []
  );

  const verifyRoomDirectoryAuthority = useCallback(
    async (actual: RoomDirectoryAuthority): Promise<void> => {
      await resolveTrustedRoomDirectoryAuthority(actual);
    },
    [resolveTrustedRoomDirectoryAuthority]
  );

  const fetchVerifiedRoomDirectory = useCallback(async () => {
    const payload = await fetchRooms(true);
    const trustedSurface = await resolveTrustedRoomDirectoryAuthority(payload);
    await bindRoomDirectoryAuthority(payload, trustedSurface);
    return payload;
  }, [resolveTrustedRoomDirectoryAuthority]);

  const refreshRoomDirectory = useCallback(async () => {
    const payload = await fetchVerifiedRoomDirectory();
    const synchronized = commit((current) => mergeServerRoomsIntoDock(
      current,
      payload.rooms,
      window.location.origin,
      payload.server_id
    ));
    publishSyncIssue(null);
    return synchronized;
  }, [commit, fetchVerifiedRoomDirectory, publishSyncIssue]);

  const resolveManagerRoomAuthority = useCallback(
    (roomDockId: string): DesktopManagerRoomAuthority => {
      const bound = currentRoomDirectoryAuthority();
      const retained = authorityRef.current;
      const matches = roomsRef.current.filter((room) => room.id === roomDockId);
      const room = matches.length === 1 ? matches[0] : null;
      if (
        !hostEnabledRef.current ||
        syncIssueRef.current ||
        !bound ||
        !retained ||
        bound.server_id !== retained.server_id ||
        bound.authority_lineage_id !== retained.authority_lineage_id ||
        !room ||
        room.roomOrigin !== "local" ||
        room.connectionState !== "local" ||
        room.serverId !== bound.server_id ||
        !room.roomUid
      ) {
        throw new Error("현재 확인된 로컬 방 관리자 권위가 없습니다.");
      }
      return parseDesktopManagerRoomAuthority({
        server_id: bound.server_id,
        authority_lineage_id: bound.authority_lineage_id,
        room_id: room.meetingId,
        room_uid: room.roomUid,
      });
    },
    []
  );

  useEffect(() => {
    const persistedRooms = rooms.map(persistableRoom);
    if (hostEnabled) {
      persistRoomDockItems(persistedRooms);
      return;
    }
    syncNativeRoomDockItems(persistedRooms);
  }, [hostEnabled, rooms]);

  useEffect(() => {
    if (!hostEnabled) {
      publishSyncIssue(null);
      return;
    }
    publishSyncIssue({
      category: "room_directory_unconfirmed",
      message: "Room directory is waiting for server confirmation.",
    });
    const hydrationEpoch = hydrationEpochRef.current + 1;
    hydrationEpochRef.current = hydrationEpoch;
    let cancelled = false;
    const canPublish = () =>
      !cancelled && hydrationEpochRef.current === hydrationEpoch;
    const applyHydration = (
      payload: Awaited<ReturnType<typeof fetchRooms>>,
      capturedMembershipRevision: number,
      capturedMetadataRevision: number
    ) => {
      if (!canPublish()) return;
      if (
        membershipRevisionRef.current !== capturedMembershipRevision ||
        metadataRevisionRef.current !== capturedMetadataRevision
      ) {
        const retryMembershipRevision = membershipRevisionRef.current;
        const retryMetadataRevision = metadataRevisionRef.current;
        fetchVerifiedRoomDirectory()
          .then((retryPayload) => {
            if (!canPublish()) return;
            if (
              membershipRevisionRef.current !== retryMembershipRevision ||
              metadataRevisionRef.current !== retryMetadataRevision
            ) {
              return;
            }
            commit((current) => mergeServerRoomsIntoDock(
              current,
              retryPayload.rooms,
              window.location.origin,
              retryPayload.server_id
            ));
            publishSyncIssue(null);
          })
          .catch((errorValue) => {
            if (!canPublish()) return;
            publishSyncIssue({
              category: "room_directory_unavailable",
              message:
                errorValue instanceof Error
                  ? errorValue.message
                  : "Room directory synchronization failed.",
            });
          });
        return;
      }
      commit((current) => mergeServerRoomsIntoDock(
        current,
        payload.rooms,
        window.location.origin,
        payload.server_id
      ));
      publishSyncIssue(null);
    };
    const capturedMembershipRevision = membershipRevisionRef.current;
    const capturedMetadataRevision = metadataRevisionRef.current;
    fetchVerifiedRoomDirectory()
      .then((payload) =>
        applyHydration(
          payload,
          capturedMembershipRevision,
          capturedMetadataRevision
        )
      )
      .catch((errorValue) => {
        if (!canPublish()) return;
        publishSyncIssue({
          category: "room_directory_unavailable",
          message:
            errorValue instanceof Error
              ? errorValue.message
              : "Room directory synchronization failed.",
        });
      });
    return () => {
      cancelled = true;
      hydrationEpochRef.current += 1;
    };
  }, [commit, fetchVerifiedRoomDirectory, hostEnabled, publishSyncIssue]);

  return {
    rooms,
    replaceRooms,
    prependRoom,
    mergeFlowRoom,
    markRoomRead,
    removeRoom,
    updateRoom,
    updateRoomByMeetingId,
    refreshRoomDirectory,
    verifyRoomDirectoryAuthority,
    resolveManagerRoomAuthority,
    syncIssue,
  };
}
