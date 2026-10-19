import { useCallback, useEffect, useRef, useState } from "react";
import { openRoomSocket, type RoomSocketHandle } from "../roomSocketClient";
import type { RoomSocketSnapshot } from "../roomSocketTypes";
import type { RoomEvent } from "../api";
import { memberAdmissionFreeSelect, type SecureMemberAdmission } from "../lib/central/secureMemberEntry";
import type { RoomInviteJoinResponse } from "../lib/roomAdmissionContract";
import {
  MEMBER_ROOM_LIVE_SOCKETS_PER_HOST,
  memberRoomLiveKey,
  pickLiveRoomIds,
  projectMemberRoomBadge,
  type MemberRoomLiveBadge,
} from "../lib/central/memberRoomLive";
import { noteMemberRoomSessionAuthFailed } from "./useMemberRoomAutoReselect";
import { tryClaimMemberRoomAutoReselect, tryClaimMemberRoomAutoReselectRetry } from "../lib/central/memberRoomAutoReselectBudget";
import type { MemberHostSnapshot } from "./useMemberHosts";

export type { MemberRoomLiveBadge };
export { MEMBER_ROOM_LIVE_SOCKETS_PER_HOST, memberRoomLiveKey };

type OpenSocket = typeof openRoomSocket;

type RoomRecord = {
  serverId: string;
  meetingId: string;
  admission: SecureMemberAdmission;
  session: RoomInviteJoinResponse | null;
  socket: RoomSocketHandle | null;
  // A background select already in flight for this room. Snapshot churn
  // re-runs the effect while the first select still awaits its central
  // grant, so without this a second select would spend a second grant for
  // the same room and invalidate the first flow.
  selecting: boolean;
  floor: number;
  floored: boolean;
  needsFreshFloor: boolean;
  latest: number;
  viewer: { participantId: string; displayName: string };
  unread: boolean;
  mention: boolean;
  activity: number;
  flow: number;
  // Automatic re-selects after a dead room session, per admission. One
  // retry heals an expired session; a persistently dead one parks instead
  // of spinning grants, and selecting the room still works on demand.
  reauth: number;
};

/**
 * Live background subscriptions for joined member rooms. After attach each
 * covered room is selected on its host (adopting the channel) and keeps one
 * lightweight room event stream over the SAME host channel — the existing
 * per-room socket path, no new wire format. Tracks only rail state (unread,
 * mention, connection); the canonical socket owns the active room.
 */
export function useMemberRoomLive({
  snapshots,
  admissionFor,
  activeServerId,
  activeMeetingId,
  accountId = "",
  openSocket = openRoomSocket,
}: {
  snapshots: MemberHostSnapshot[];
  admissionFor: (serverId: string) => SecureMemberAdmission | null | undefined;
  activeServerId: string;
  activeMeetingId: string;
  accountId?: string;
  openSocket?: OpenSocket;
}) {
  const [badges, setBadges] = useState<Record<string, MemberRoomLiveBadge>>({});
  const [tick, setTick] = useState(0);
  const records = useRef(new Map<string, RoomRecord>());
  const openSocketRef = useRef(openSocket);
  openSocketRef.current = openSocket;
  const admissionForRef = useRef(admissionFor);
  admissionForRef.current = admissionFor;

  const clearBadge = useCallback((key: string) => {
    setBadges((previous) => {
      if (!previous[key]) return previous;
      const next = { ...previous };
      delete next[key];
      return next;
    });
  }, []);

  const publishBadge = useCallback((key: string, badge: MemberRoomLiveBadge) => {
    setBadges((previous) => {
      const current = previous[key];
      if (current && current.unread === badge.unread && current.mention === badge.mention) return previous;
      if (!badge.unread && !badge.mention) {
        if (!current) return previous;
        const next = { ...previous };
        delete next[key];
        return next;
      }
      return { ...previous, [key]: badge };
    });
  }, []);

  // Instant switch: a live room's cached session opens without another
  // admission round trip. Null when the host channel is gone (the normal
  // select path re-admits instead).
  const liveSessionFor = useCallback((serverId: string, meetingId: string): RoomInviteJoinResponse | null => {
    const record = records.current.get(memberRoomLiveKey(serverId, meetingId));
    if (!record?.session) return null;
    const admission = admissionForRef.current(serverId);
    if (!admission || admission !== record.admission || !admission.transport.active) return null;
    return record.session;
  }, []);

  const hasLive = useCallback((serverId: string, meetingId: string): boolean => {
    return records.current.has(memberRoomLiveKey(serverId, meetingId));
  }, []);

  const markRoomRead = useCallback((serverId: string, meetingId: string) => {
    const key = memberRoomLiveKey(serverId, meetingId);
    const record = records.current.get(key);
    if (record) {
      record.floor = record.latest;
      record.unread = false;
      record.mention = false;
    }
    clearBadge(key);
  }, [clearBadge]);

  useEffect(() => {
    const seen = new Set<string>();
    const activeKey = activeServerId && activeMeetingId
      ? memberRoomLiveKey(activeServerId, activeMeetingId)
      : "";

    for (const snapshot of snapshots) {
      if (snapshot.phase !== "connected" || !snapshot.rooms) {
        // Host offline: its rooms show disconnected with no counts (unknown
        // stays unknown). Park the records (floors/sessions kept) and drop
        // badges; delete nothing so reconnect resubscribes from the floor.
        for (const [key, record] of records.current) {
          if (record.serverId !== snapshot.serverId) continue;
          record.socket?.close();
          record.socket = null;
          record.flow += 1;
          record.selecting = false;
          record.unread = false;
          record.mention = false;
          clearBadge(key);
        }
        continue;
      }
      const admission = admissionForRef.current(snapshot.serverId);
      const liveRooms = snapshot.rooms.map((room) => room.room_id);
      const byActivity = [...liveRooms].sort((left, right) => {
        const leftActivity = records.current.get(memberRoomLiveKey(snapshot.serverId, left))?.activity || 0;
        const rightActivity = records.current.get(memberRoomLiveKey(snapshot.serverId, right))?.activity || 0;
        return rightActivity - leftActivity;
      });
      const suspendedForActive = activeServerId === snapshot.serverId ? activeMeetingId : "";
      const covered = new Set(
        pickLiveRoomIds(byActivity, suspendedForActive, MEMBER_ROOM_LIVE_SOCKETS_PER_HOST)
      );
      const listed = new Set(liveRooms);
      for (const room of snapshot.rooms) {
        const key = memberRoomLiveKey(snapshot.serverId, room.room_id);
        seen.add(key);
        if (key === activeKey) {
          // The canonical socket owns the active room: suspend the
          // background stream and mark it read; resubscribe takes a fresh
          // floor when the room backgrounds again.
          const record = records.current.get(key);
          if (record) {
            record.socket?.close();
            record.socket = null;
            record.flow += 1;
            record.selecting = false;
            record.floor = record.latest;
            record.unread = false;
            record.mention = false;
            record.needsFreshFloor = true;
          }
          clearBadge(key);
          continue;
        }
        if (!covered.has(room.room_id)) {
          const record = records.current.get(key);
          if (record) {
            record.socket?.close();
            record.socket = null;
            record.flow += 1;
            record.selecting = false;
            record.unread = false;
            record.mention = false;
          }
          clearBadge(key);
          continue;
        }
        let record = records.current.get(key);
        if (!record) {
          // Without a host admission there is no channel to select on yet
          // (first-join adopt path); the first room select creates one and
          // the background covers the rest from then on.
          if (!admission) continue;
          record = {
            serverId: snapshot.serverId,
            meetingId: room.room_id,
            admission,
            session: null,
            socket: null,
            selecting: false,
            floor: 0,
            floored: false,
            needsFreshFloor: false,
            latest: 0,
            viewer: { participantId: "", displayName: "" },
            unread: false,
            mention: false,
            activity: Date.now(),
            flow: 0,
            reauth: 0,
          };
          records.current.set(key, record);
        } else if (admission && record.admission !== admission) {
          // Host re-attached on a fresh channel: cached sessions were bound
          // to the dead transport, so drop them and re-select below.
          record.admission = admission;
          record.session = null;
          record.socket?.close();
          record.socket = null;
          record.flow += 1;
          record.selecting = false;
          record.reauth = 0;
        }
        const liveAdmission = record.admission;
        if (record.socket || record.selecting || !liveAdmission.transport.active) continue;
        // Background re-selects share the automatic budget with the
        // foreground re-select: at most one per fresh EXTERNAL host
        // re-attach and a daily cap per host+room. Internal re-attaches
        // (same host generation) and an exhausted budget park silently here
        // — no grant is spent, and the rail keeps last-known state. A
        // dead-session heal already spent its extra claim in onError, so it
        // proceeds without a second generation claim.
        if (!record.session && record.reauth === 0) {
          if (!tryClaimMemberRoomAutoReselect(accountId, snapshot.serverId, room.room_id).allowed) continue;
        }
        const flow = record.flow + 1;
        record.flow = flow;
        record.selecting = true;
        const current = record;
        void (async () => {
          try {
            let session = current.session;
            if (!session) {
              if (admissionForRef.current(snapshot.serverId) !== liveAdmission || !liveAdmission.transport.active) return;
              session = await liveAdmission.selectBackgroundRoom(room.room_id);
              if (records.current.get(key) !== current || current.flow !== flow) return;
              current.session = session;
              current.viewer = { participantId: session.agent_id, displayName: session.display_name };
            }
            if (records.current.get(key) !== current || current.flow !== flow) return;
            if (admissionForRef.current(snapshot.serverId) !== liveAdmission || !liveAdmission.transport.active) return;
            const live = current;
            const auth = { kind: "session" as const, sessionToken: session.session_token, deviceToken: liveAdmission.deviceToken };
            const handle = openSocketRef.current(auth, session.server_product_surface.websocket_streams, {
              onRoomSnapshot: (snapshotBody: RoomSocketSnapshot) => {
                if (records.current.get(key) !== live || live.flow !== flow) return false;
                if (String(snapshotBody.room?.room_id || "") !== live.meetingId) return false;
                if (live.needsFreshFloor || !live.floored) {
                  live.floor = Number(snapshotBody.last_seq) || 0;
                  live.floored = true;
                  live.needsFreshFloor = false;
                }
                live.latest = Math.max(live.latest, Number(snapshotBody.last_seq) || 0);
                const { badge, latestSeq } = projectMemberRoomBadge({
                  events: (snapshotBody.events || []) as RoomEvent[],
                  floorSeq: live.floor,
                  viewer: live.viewer,
                });
                live.latest = Math.max(live.latest, latestSeq);
                live.activity = Date.now();
                live.unread = badge.unread;
                live.mention = badge.mention;
                live.reauth = 0;
                publishBadge(key, badge);
                return true;
              },
              onRoomEvents: (events: RoomEvent[]) => {
                if (records.current.get(key) !== live || live.flow !== flow) return;
                const { badge, latestSeq } = projectMemberRoomBadge({
                  events,
                  floorSeq: live.floor,
                  viewer: live.viewer,
                });
                live.latest = Math.max(live.latest, latestSeq);
                live.activity = Date.now();
                live.unread = live.unread || badge.unread;
                live.mention = live.mention || badge.mention;
                publishBadge(key, { unread: live.unread, mention: live.mention });
              },
              onError: (error: Event | Error) => {
                if (records.current.get(key) !== live || live.flow !== flow) return;
                const category = error instanceof Error && "category" in error
                  ? String((error as { category: unknown }).category)
                  : "";
                if (category !== "authorization_failed") return;
                // The room session is dead (revoked/expired): poison its bearer
                // so a later tap or heal never re-installs it, then drop it
                // and re-select once through a fresh admission cycle. A
                // persistently dead session parks instead of spinning.
                const dead = live.session;
                if (dead) noteMemberRoomSessionAuthFailed(dead.session_token);
                live.session = null;
                live.socket?.close();
                live.socket = null;
                live.flow += 1;
                if (live.reauth < 1 && admissionForRef.current(snapshot.serverId) === liveAdmission &&
                    liveAdmission.transport.active) {
                  // A free channel-bound heal spends no budget so hourly
                  // expiries never park; a paid heal claims one retry grant.
                  if (memberAdmissionFreeSelect(liveAdmission)) {
                    live.reauth += 1;
                    setTick((value) => value + 1);
                  } else if (tryClaimMemberRoomAutoReselectRetry(accountId, snapshot.serverId, live.meetingId).allowed) {
                    live.reauth += 1;
                    setTick((value) => value + 1);
                  }
                }
              },
            }, {
              serverSurface: session.server_product_surface,
              expectedRoomId: session.meeting_id,
              expectedParticipantId: session.agent_id,
            });
            if (records.current.get(key) !== live || live.flow !== flow) {
              handle.close();
              return;
            }
            live.socket = handle;
          } catch {
            // A failed background select stays silent on the rail (last-known
            // rooms, host connection state) and retries on the next snapshot
            // or host change; selecting the room still works on demand.
            if (records.current.get(key) === current && current.flow === flow) {
              current.socket = null;
            }
          } finally {
            if (records.current.get(key) === current && current.flow === flow) {
              current.selecting = false;
            }
          }
        })();
      }
      // The live host confirms its listing: rooms it no longer lists are
      // genuinely removed, so their records (and sessions) go, no tombstone.
      for (const [key, record] of [...records.current]) {
        if (record.serverId !== snapshot.serverId || seen.has(key)) continue;
        if (listed.has(record.meetingId)) continue;
        record.socket?.close();
        records.current.delete(key);
        clearBadge(key);
      }
    }
    // Hosts gone from the directory entirely drop their records.
    const liveHosts = new Set(snapshots.map((snapshot) => snapshot.serverId));
    for (const [key, record] of [...records.current]) {
      if (liveHosts.has(record.serverId) || seen.has(key)) continue;
      record.socket?.close();
      records.current.delete(key);
      clearBadge(key);
    }
    return () => {
      // Reconcile runs again on the next change; in-flight flows check the
      // per-room generation and land on nothing stale.
    };
  }, [snapshots, activeServerId, activeMeetingId, accountId, tick, publishBadge, clearBadge]);

  useEffect(() => () => {
    for (const record of records.current.values()) {
      record.socket?.close();
      record.flow += 1;
    }
    records.current.clear();
  }, []);

  return { badges, liveSessionFor, hasLive, markRoomRead };
}
