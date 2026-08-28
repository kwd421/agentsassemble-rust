import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
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
  type StrictRoomDirectory,
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

const UNCONFIRMED_ISSUE: RoomDirectorySyncIssue = {
  category: "room_directory_unconfirmed",
  message: "Room directory is waiting for server confirmation.",
};

const continuityBrand: unique symbol = Symbol("room-directory-continuity");

export type RoomDirectoryContinuity = Readonly<{
  readonly [continuityBrand]: true;
}>;

type InternalContinuity = RoomDirectoryContinuity & {
  owner: object;
  epoch: number;
};

export class RoomDirectoryOperationSuperseded extends Error {
  constructor() {
    super("방 목록 작업이 현재 권위에 의해 대체되었습니다.");
    this.name = "RoomDirectoryOperationSuperseded";
  }
}

export type RoomDirectoryVerificationResult =
  | { ok: true; continuity: RoomDirectoryContinuity }
  | { ok: false; continuity: RoomDirectoryContinuity; error: unknown };

export type RoomDirectoryRefreshResult =
  | { ok: true; continuity: RoomDirectoryContinuity; rooms: RoomDockItem[] }
  | { ok: false; continuity: RoomDirectoryContinuity; error: unknown };

type ManagerAuthoritySnapshot = {
  epoch: number;
  authority: RoomDirectoryAuthority;
  byDockId: ReadonlyMap<string, DesktopManagerRoomAuthority>;
};

function frozenContinuity(owner: object, epoch: number): RoomDirectoryContinuity {
  return Object.freeze({
    [continuityBrand]: true as const,
    owner,
    epoch,
  }) as InternalContinuity;
}

function managerAuthoritySnapshot(
  rooms: RoomDockItem[],
  payload: StrictRoomDirectory,
  epoch: number
): ManagerAuthoritySnapshot {
  const dockIdCounts = new Map<string, number>();
  for (const room of rooms) {
    dockIdCounts.set(room.id, (dockIdCounts.get(room.id) || 0) + 1);
  }
  const activePayloadByRoomId = new Map<
    string,
    StrictRoomDirectory["rooms"][number]
  >();
  for (const room of payload.rooms) {
    if (!room.archived && room.status !== "closed" && room.status !== "archived") {
      activePayloadByRoomId.set(room.room_id, room);
    }
  }
  const associatedPayloads = new Set<string>();
  const byDockId = new Map<string, DesktopManagerRoomAuthority>();
  for (const room of rooms) {
    if (
      room.roomOrigin !== "local" ||
      room.connectionState !== "local" ||
      room.serverId !== payload.server_id ||
      !room.roomUid
    ) {
      continue;
    }
    if (dockIdCounts.get(room.id) !== 1) {
      throw new Error("확인된 로컬 방 dock 식별자가 중복되었습니다.");
    }
    const matched = activePayloadByRoomId.get(room.meetingId);
    if (!matched || matched.room_uid !== room.roomUid) {
      throw new Error("로컬 방 dock과 권위 있는 방 목록의 연결이 모호합니다.");
    }
    const payloadKey = `${matched.room_id}\0${matched.room_uid}`;
    if (associatedPayloads.has(payloadKey) || byDockId.has(room.id)) {
      throw new Error("하나의 방 권위가 여러 로컬 dock에 연결되었습니다.");
    }
    associatedPayloads.add(payloadKey);
    byDockId.set(
      room.id,
      parseDesktopManagerRoomAuthority({
        server_id: payload.server_id,
        authority_lineage_id: payload.authority_lineage_id,
        room_id: matched.room_id,
        room_uid: matched.room_uid || "",
      })
    );
  }
  return Object.freeze({
    epoch,
    authority: {
      server_id: payload.server_id,
      authority_lineage_id: payload.authority_lineage_id,
    },
    byDockId,
  });
}

export function useRoomDirectory({
  initialRooms,
  hostEnabled,
}: UseRoomDirectoryOptions) {
  const initialIssue = hostEnabled ? UNCONFIRMED_ISSUE : null;
  const roomsRef = useRef<RoomDockItem[]>(initialRooms);
  const [rooms, setRooms] = useState<RoomDockItem[]>(initialRooms);
  const activeRef = useRef(false);
  const hostEnabledRef = useRef(false);
  const continuityOwnerRef = useRef<object>({});
  const publicationEpochRef = useRef(0);
  const reservedHydrationEpochRef = useRef<number | null>(null);
  const managerSnapshotRef = useRef<ManagerAuthoritySnapshot | null>(null);
  const syncIssueRef = useRef<RoomDirectorySyncIssue | null>(initialIssue);
  const [syncIssue, setSyncIssueState] =
    useState<RoomDirectorySyncIssue | null>(initialIssue);
  const membershipRevisionRef = useRef(0);
  const metadataRevisionRef = useRef(0);
  const authorityRef = useRef<RoomDirectoryAuthority | null>(
    currentRoomDirectoryAuthority()
  );

  const publishSyncIssue = useCallback((issue: RoomDirectorySyncIssue | null) => {
    syncIssueRef.current = issue;
    setSyncIssueState(issue);
  }, []);

  const invalidateDirectory = useCallback(() => {
    publicationEpochRef.current += 1;
    reservedHydrationEpochRef.current = null;
    managerSnapshotRef.current = null;
  }, []);

  const isCurrentEpoch = useCallback(
    (epoch: number) =>
      activeRef.current &&
      hostEnabledRef.current &&
      publicationEpochRef.current === epoch,
    []
  );

  const assertCurrentEpoch = useCallback(
    (epoch: number) => {
      if (!isCurrentEpoch(epoch)) throw new RoomDirectoryOperationSuperseded();
    },
    [isCurrentEpoch]
  );

  const validateRoomDirectoryContinuity = useCallback(
    (continuity: RoomDirectoryContinuity) => {
      const candidate = continuity as InternalContinuity;
      if (
        candidate?.[continuityBrand] !== true ||
        candidate.owner !== continuityOwnerRef.current
      ) {
        throw new RoomDirectoryOperationSuperseded();
      }
      assertCurrentEpoch(candidate.epoch);
    },
    [assertCurrentEpoch]
  );

  const captureRoomDirectoryContinuity = useCallback(() => {
    assertCurrentEpoch(publicationEpochRef.current);
    return frozenContinuity(
      continuityOwnerRef.current,
      publicationEpochRef.current
    );
  }, [assertCurrentEpoch]);

  const beginDirectoryOperation = useCallback(
    (predecessor: RoomDirectoryContinuity) => {
      validateRoomDirectoryContinuity(predecessor);
      publicationEpochRef.current += 1;
      reservedHydrationEpochRef.current = null;
      managerSnapshotRef.current = null;
      publishSyncIssue(UNCONFIRMED_ISSUE);
      return publicationEpochRef.current;
    },
    [publishSyncIssue, validateRoomDirectoryContinuity]
  );

  const continuityForEpoch = useCallback(
    (epoch: number) => {
      assertCurrentEpoch(epoch);
      return frozenContinuity(continuityOwnerRef.current, epoch);
    },
    [assertCurrentEpoch]
  );

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
    async (actual: RoomDirectoryAuthority) => {
      if (isDesktopWebview()) {
        const bootstrap = await requestDesktopBootstrapStatus();
        if (bootstrap.phase !== "complete") {
          throw new Error("완료된 데스크톱 bootstrap 권위가 없습니다.");
        }
        return {
          retainedAuthority: retainRoomDirectoryAuthority(
            actual,
            authorityRef.current,
            bootstrap
          ),
          trustedSurface: {
            revision: bootstrap.server_product_surface_revision,
            digest: bootstrap.server_product_surface_digest,
          } satisfies TrustedServerProductSurface,
        };
      }
      return {
        retainedAuthority: retainRoomDirectoryAuthority(actual, authorityRef.current),
        trustedSurface: null,
      };
    },
    []
  );

  const verifyRoomDirectoryAuthority = useCallback(
    async (
      actual: RoomDirectoryAuthority,
      predecessor: RoomDirectoryContinuity
    ): Promise<RoomDirectoryVerificationResult> => {
      const epoch = beginDirectoryOperation(predecessor);
      try {
        await resolveTrustedRoomDirectoryAuthority(actual);
        return { ok: true, continuity: continuityForEpoch(epoch) };
      } catch (error) {
        if (!isCurrentEpoch(epoch)) throw new RoomDirectoryOperationSuperseded();
        publishSyncIssue({
          category: "room_directory_unavailable",
          message: error instanceof Error ? error.message : "Room directory verification failed.",
        });
        return { ok: false, continuity: continuityForEpoch(epoch), error };
      }
    },
    [
      beginDirectoryOperation,
      continuityForEpoch,
      isCurrentEpoch,
      publishSyncIssue,
      resolveTrustedRoomDirectoryAuthority,
    ]
  );

  const fetchVerifiedRoomDirectory = useCallback(
    async (epoch: number) => {
      const payload = await fetchRooms(true, () => assertCurrentEpoch(epoch));
      assertCurrentEpoch(epoch);
      const stagedTrust = await resolveTrustedRoomDirectoryAuthority(payload);
      assertCurrentEpoch(epoch);
      const bound = await bindRoomDirectoryAuthority(
        payload,
        stagedTrust.trustedSurface,
        window.location.origin,
        () => isCurrentEpoch(epoch)
      );
      if (!bound) throw new RoomDirectoryOperationSuperseded();
      assertCurrentEpoch(epoch);
      authorityRef.current = stagedTrust.retainedAuthority;
      return payload;
    },
    [assertCurrentEpoch, isCurrentEpoch, resolveTrustedRoomDirectoryAuthority]
  );

  const publishDirectory = useCallback(
    (payload: StrictRoomDirectory, epoch: number) => {
      const synchronized = mergeServerRoomsIntoDock(
        roomsRef.current,
        payload.rooms,
        window.location.origin,
        payload.server_id
      );
      const snapshot = managerAuthoritySnapshot(synchronized, payload, epoch);
      assertCurrentEpoch(epoch);
      roomsRef.current = synchronized;
      managerSnapshotRef.current = snapshot;
      setRooms(synchronized);
      publishSyncIssue(null);
      return synchronized;
    },
    [assertCurrentEpoch, publishSyncIssue]
  );

  const refreshRoomDirectory = useCallback(
    async (predecessor: RoomDirectoryContinuity): Promise<RoomDirectoryRefreshResult> => {
      const epoch = beginDirectoryOperation(predecessor);
      try {
        const payload = await fetchVerifiedRoomDirectory(epoch);
        const synchronized = publishDirectory(payload, epoch);
        return {
          ok: true,
          rooms: synchronized,
          continuity: continuityForEpoch(epoch),
        };
      } catch (error) {
        if (!isCurrentEpoch(epoch)) throw new RoomDirectoryOperationSuperseded();
        publishSyncIssue({
          category: "room_directory_unavailable",
          message: error instanceof Error ? error.message : "Room directory synchronization failed.",
        });
        return { ok: false, continuity: continuityForEpoch(epoch), error };
      }
    },
    [
      beginDirectoryOperation,
      continuityForEpoch,
      fetchVerifiedRoomDirectory,
      isCurrentEpoch,
      publishDirectory,
      publishSyncIssue,
    ]
  );

  const resolveManagerRoomAuthority = useCallback(
    (roomDockId: string): DesktopManagerRoomAuthority => {
      const snapshot = managerSnapshotRef.current;
      const bound = currentRoomDirectoryAuthority();
      const matches = roomsRef.current.filter((room) => room.id === roomDockId);
      const room = matches.length === 1 ? matches[0] : null;
      const frozen = snapshot?.byDockId.get(roomDockId);
      if (
        !snapshot ||
        snapshot.epoch !== publicationEpochRef.current ||
        !activeRef.current ||
        !hostEnabledRef.current ||
        syncIssueRef.current ||
        !bound ||
        bound.server_id !== snapshot.authority.server_id ||
        bound.authority_lineage_id !== snapshot.authority.authority_lineage_id ||
        !room ||
        !frozen ||
        room.roomOrigin !== "local" ||
        room.connectionState !== "local" ||
        room.serverId !== frozen.server_id ||
        room.meetingId !== frozen.room_id ||
        room.roomUid !== frozen.room_uid
      ) {
        throw new Error("현재 확인된 로컬 방 관리자 권위가 없습니다.");
      }
      return frozen;
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

  useLayoutEffect(() => {
    activeRef.current = true;
    hostEnabledRef.current = hostEnabled;
    publicationEpochRef.current += 1;
    managerSnapshotRef.current = null;
    reservedHydrationEpochRef.current = hostEnabled
      ? publicationEpochRef.current
      : null;
    publishSyncIssue(hostEnabled ? UNCONFIRMED_ISSUE : null);
    return () => {
      activeRef.current = false;
      hostEnabledRef.current = false;
      invalidateDirectory();
    };
  }, [hostEnabled, invalidateDirectory, publishSyncIssue]);

  useEffect(() => {
    if (!hostEnabled) return;
    const epoch = reservedHydrationEpochRef.current;
    reservedHydrationEpochRef.current = null;
    if (epoch === null || !isCurrentEpoch(epoch)) return;
    const capturedMembershipRevision = membershipRevisionRef.current;
    const capturedMetadataRevision = metadataRevisionRef.current;

    const hydrate = async () => {
      let payload = await fetchVerifiedRoomDirectory(epoch);
      if (
        membershipRevisionRef.current !== capturedMembershipRevision ||
        metadataRevisionRef.current !== capturedMetadataRevision
      ) {
        const retryMembershipRevision = membershipRevisionRef.current;
        const retryMetadataRevision = metadataRevisionRef.current;
        payload = await fetchVerifiedRoomDirectory(epoch);
        assertCurrentEpoch(epoch);
        if (
          membershipRevisionRef.current !== retryMembershipRevision ||
          metadataRevisionRef.current !== retryMetadataRevision
        ) {
          return;
        }
      }
      publishDirectory(payload, epoch);
    };

    hydrate().catch((error) => {
      if (!isCurrentEpoch(epoch)) return;
      publishSyncIssue({
        category: "room_directory_unavailable",
        message: error instanceof Error ? error.message : "Room directory synchronization failed.",
      });
    });
  }, [
    assertCurrentEpoch,
    fetchVerifiedRoomDirectory,
    hostEnabled,
    isCurrentEpoch,
    publishDirectory,
    publishSyncIssue,
  ]);

  return {
    rooms,
    replaceRooms,
    prependRoom,
    mergeFlowRoom,
    markRoomRead,
    removeRoom,
    updateRoom,
    updateRoomByMeetingId,
    captureRoomDirectoryContinuity,
    validateRoomDirectoryContinuity,
    refreshRoomDirectory,
    verifyRoomDirectoryAuthority,
    resolveManagerRoomAuthority,
    syncIssue,
  };
}
