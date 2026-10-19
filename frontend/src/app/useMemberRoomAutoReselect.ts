import { useEffect, useRef } from "react";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import { remoteSessionTransport, sessionHostOrigin } from "../lib/remote/remoteWorkspace";
import {
  MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE,
  tryClaimMemberRoomAutoReselect,
  tryClaimMemberRoomAutoReselectRetry,
} from "../lib/central/memberRoomAutoReselectBudget";
import { memberAdmissionFreeSelect } from "../lib/central/secureMemberEntry";
import { memberAdmissionServerFor } from "./remoteRoomAuthority";

/**
 * Open room follows its host across a restart: when the member host
 * re-attaches on a fresh admission (new origin/channel) the previously
 * selected room session is bound to the dead channel and the new host
 * refuses it. Re-select exactly once per admission through the new
 * admission so the socket and workspace switch to the fresh bearer; a
 * refused re-select parks with the inline Korean failure instead of
 * looping. The same path heals sign-in: a joined host that is connected
 * but has no live room session opens within seconds without user action.
 * The trigger is a NEW admission for this room only: a missing session on
 * an unchanged admission never auto-retries (a manual failure already
 * surfaced its inline error and waits for user action or a fresh host
 * channel), so failed taps stay bounded at one attempt per admission.
 * A session loss on the SAME live admission (expiry or authorization loss
 * while the host channel stays live) heals once below through the same
 * admission owner: a free channel-bound select spends no budget, a paid
 * select claims one retry grant; a refused room parks instead of looping.
 */
export function useMemberRoomAutoReselect({
  activeRoom,
  ownerServerId,
  accountId = "",
  hasAnchor,
  isRefused,
  snapshots,
  admissionFor,
  guestSession,
  guestExpired,
  admittedSessionToken,
  liveSessionFor,
  selectRoom,
  acceptSession,
  setSelectError,
}: {
  activeRoom: RoomDockItem;
  ownerServerId: string | undefined;
  accountId?: string;
  hasAnchor: (serverId: string) => boolean;
  isRefused: (serverId: string) => boolean;
  snapshots: { serverId: string; phase: string; origin: string }[];
  admissionFor: (serverId: string) => unknown;
  guestSession: RoomGuestSession | null;
  guestExpired: boolean;
  admittedSessionToken: string;
  liveSessionFor: (serverId: string, meetingId: string) => unknown;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  selectRoom: (serverId: string, meetingId: string) => Promise<any>;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  acceptSession: (payload: any) => Promise<boolean>;
  setSelectError: (error: { roomId: string; message: string } | null | ((current: { roomId: string; message: string } | null) => { roomId: string; message: string } | null)) => void;
}) {
  const seen = useRef(new Map<string, unknown>());
  const flight = useRef(false);
  const markSeen = useRef((serverId: string, meetingId: string) => {
    try {
      seen.current.set(`${serverId}\0${meetingId}`, admissionFor(serverId) ?? null);
    } catch {
      // Admission bookkeeping is best-effort; the inline error already bounds the tap.
    }
  });

  useEffect(() => {
    if (activeRoom.roomOrigin !== "remote_server" || !activeRoom.serverId) return;
    const memberServerId = memberAdmissionServerFor(activeRoom, ownerServerId, hasAnchor);
    if (!memberServerId || isRefused(memberServerId)) return;
    const snapshot = snapshots.find((entry) => entry.serverId === memberServerId);
    if (!snapshot || snapshot.phase !== "connected") return;
    const admission = admissionFor(memberServerId);
    if (!admission) return;
    const activeKey = `${memberServerId}\0${activeRoom.meetingId}`;
    if (seen.current.get(activeKey) === admission) return;
    const sessionReadyForRoom = Boolean(
      guestSession && !guestExpired && admittedSessionToken &&
      guestSession.meetingId === activeRoom.meetingId &&
      guestSession.serverSurface.server_id === activeRoom.serverId);
    const boundTransport = admittedSessionToken ? remoteSessionTransport(admittedSessionToken) : undefined;
    const boundOrigin = admittedSessionToken ? sessionHostOrigin(admittedSessionToken) : null;
    const staleSession = sessionReadyForRoom && (!boundTransport || !boundTransport.active ||
      (Boolean(boundOrigin) && Boolean(snapshot.origin) && boundOrigin !== snapshot.origin));
    if (sessionReadyForRoom && !staleSession) {
      seen.current.set(activeKey, admission);
      return;
    }
    if (flight.current) return;
    // Bounded automatic attempt: at most one per fresh EXTERNAL host
    // re-attach (internal re-attaches inside selectRoom share the host
    // generation and never release another claim) and at most a daily cap
    // per host+room. The budget is module-level per account, so remounts
    // never reset it and persistent 401/429/5xx parks instead of spinning
    // central grants.
    const claim = tryClaimMemberRoomAutoReselect(accountId, memberServerId, activeRoom.meetingId);
    seen.current.set(activeKey, admission);
    if (!claim.allowed) {
      if (claim.reason === "exhausted") {
        (setSelectError as (error: { roomId: string; message: string }) => void)(
          { roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
      }
      return;
    }
    if (import.meta.env.DEV) console.debug("[member-select] auto re-select open room on fresh admission", {
      roomId: activeRoom.id, meetingId: activeRoom.meetingId, serverId: memberServerId,
      sessionReady: sessionReadyForRoom, stale: staleSession,
    });
    flight.current = true;
    void (async () => {
      try {
        const live = liveSessionFor(memberServerId, activeRoom.meetingId) as {
          meeting_id?: string;
        } | null;
        if (live) {
          try {
            if (await acceptSession(live)) {
              (setSelectError as (update: (current: { roomId: string; message: string } | null) => { roomId: string; message: string } | null) => void)(
                (current) => (current?.roomId === activeRoom.id ? null : current));
              return;
            }
          } catch {
            // Fall through to a fresh select on the new admission.
          }
        }
        const payload = await selectRoom(memberServerId, activeRoom.meetingId);
        if (!await acceptSession(payload)) {
          (setSelectError as (error: { roomId: string; message: string }) => void)({ roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
        } else {
          (setSelectError as (update: (current: { roomId: string; message: string } | null) => { roomId: string; message: string } | null) => void)(
            (current) => (current?.roomId === activeRoom.id ? null : current));
        }
      } catch {
        (setSelectError as (error: { roomId: string; message: string }) => void)({ roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
      } finally {
        flight.current = false;
      }
    })();
  }, [activeRoom, ownerServerId, accountId, hasAnchor, isRefused, snapshots, admissionFor,
    guestSession, guestExpired, admittedSessionToken, liveSessionFor,
    selectRoom, acceptSession, setSelectError]);

  // Session-loss heal on the SAME live admission. Fires only on the edge
  // from a live room session to no session (expiry or authorization loss)
  // while the host admission/transport stays live: exactly one automatic
  // re-select through the existing admission owner (the same path the
  // \uB2E4\uC2DC \uC5F0\uACB0 button uses). A free channel-bound select spends no
  // budget; a paid one claims one retry grant. A refused re-select parks
  // with the stop line and never retries until a new live session dies or
  // the user taps again. No timers or polling.
  const sessionLossKey = useRef("");
  const hadLiveSession = useRef(false);
  useEffect(() => {
    if (activeRoom.roomOrigin !== "remote_server" || !activeRoom.serverId) return;
    const memberServerId = memberAdmissionServerFor(activeRoom, ownerServerId, hasAnchor);
    if (!memberServerId || isRefused(memberServerId)) return;
    const snapshot = snapshots.find((entry) => entry.serverId === memberServerId);
    if (!snapshot || snapshot.phase !== "connected") return;
    let admission: unknown = null;
    try {
      admission = admissionFor(memberServerId);
    } catch {
      return;
    }
    if (!admission) return;
    const transportActive =
      (admission as { transport?: { active?: boolean } })?.transport?.active === true;
    if (!transportActive) return;
    const lossKey = `${memberServerId}\0${activeRoom.meetingId}`;
    const sessionReadyForRoom = Boolean(
      guestSession && !guestExpired && admittedSessionToken &&
      guestSession.meetingId === activeRoom.meetingId &&
      guestSession.serverSurface.server_id === activeRoom.serverId);
    if (lossKey !== sessionLossKey.current) {
      sessionLossKey.current = lossKey;
      hadLiveSession.current = sessionReadyForRoom;
      return;
    }
    if (sessionReadyForRoom) {
      hadLiveSession.current = true;
      return;
    }
    if (!hadLiveSession.current) return;
    if (flight.current) return;
    // Consume this loss before the async attempt so a refused room never
    // retries on the same loss; a new live session re-arms the next loss.
    hadLiveSession.current = false;
    const free = memberAdmissionFreeSelect(admission);
    if (!free) {
      const claim = tryClaimMemberRoomAutoReselectRetry(accountId, memberServerId, activeRoom.meetingId);
      if (!claim.allowed) {
        if (claim.reason === "exhausted") {
          (setSelectError as (error: { roomId: string; message: string }) => void)(
            { roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
        }
        return;
      }
    }
    if (import.meta.env.DEV) console.debug("[member-select] auto re-select open room on session loss", {
      roomId: activeRoom.id, meetingId: activeRoom.meetingId, serverId: memberServerId, free,
    });
    flight.current = true;
    void (async () => {
      try {
        const live = liveSessionFor(memberServerId, activeRoom.meetingId) as {
          meeting_id?: string;
        } | null;
        if (live) {
          try {
            if (await acceptSession(live)) {
              (setSelectError as (update: (current: { roomId: string; message: string } | null) => { roomId: string; message: string } | null) => void)(
                (current) => (current?.roomId === activeRoom.id ? null : current));
              return;
            }
          } catch {
            // Fall through to a fresh select on the live admission.
          }
        }
        const payload = await selectRoom(memberServerId, activeRoom.meetingId);
        if (!await acceptSession(payload)) {
          (setSelectError as (error: { roomId: string; message: string }) => void)({ roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
        } else {
          (setSelectError as (update: (current: { roomId: string; message: string } | null) => { roomId: string; message: string } | null) => void)(
            (current) => (current?.roomId === activeRoom.id ? null : current));
        }
      } catch {
        (setSelectError as (error: { roomId: string; message: string }) => void)({ roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
      } finally {
        flight.current = false;
      }
    })();
  }, [activeRoom, ownerServerId, accountId, hasAnchor, isRefused, snapshots, admissionFor,
    guestSession, guestExpired, admittedSessionToken, liveSessionFor,
    selectRoom, acceptSession, setSelectError]);

  return { markAdmissionSeen: markSeen.current };
}

/**
 * Manual rail tap for a member room: prefers a live background session,
 * otherwise selects once through the host admission. Records the short
 * Korean inline failure instead of silence or an alert; the caller marks
 * the consumed admission so the automatic re-select stays bounded.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function selectMemberRoomOnDemand({ roomId, meetingId, memberServerId, liveSessionFor, selectRoom, acceptSession, setSelectError }: {
  roomId: string; meetingId: string; memberServerId: string;
  liveSessionFor: (serverId: string, meetingId: string) => unknown;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  selectRoom: (serverId: string, meetingId: string) => Promise<any>;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  acceptSession: (payload: any) => Promise<boolean>;
  setSelectError: (error: { roomId: string; message: string } | null | ((current: { roomId: string; message: string } | null) => { roomId: string; message: string } | null)) => void;
}): Promise<void> {
  try {
    const live = liveSessionFor(memberServerId, meetingId);
    let opened = false;
    if (live) {
      try {
        opened = await acceptSession(live);
      } catch {
        opened = false;
      }
    }
    if (opened) {
      (setSelectError as (update: (current: { roomId: string; message: string } | null) => { roomId: string; message: string } | null) => void)(
        (current) => (current?.roomId === roomId ? null : current));
    } else {
      const payload = await selectRoom(memberServerId, meetingId);
      if (import.meta.env.DEV) console.debug("[member-select] admission complete", {
        roomId, meetingId: payload.meeting_id, serverId: payload.server_id,
      });
      if (!await acceptSession(payload)) {
        (setSelectError as (error: { roomId: string; message: string }) => void)({ roomId, message: "방을 열지 못했어요. 다시 시도해 주세요." });
      } else {
        (setSelectError as (update: (current: { roomId: string; message: string } | null) => { roomId: string; message: string } | null) => void)(
          (current) => (current?.roomId === roomId ? null : current));
      }
    }
  } catch (error) {
    if (import.meta.env.DEV) console.debug("[member-select] select failed", {
      roomId, meetingId, serverId: memberServerId,
      message: error instanceof Error ? error.message : String(error),
    });
    (setSelectError as (error: { roomId: string; message: string }) => void)({ roomId, message: "방을 열지 못했어요. 다시 시도해 주세요." });
  }
}
