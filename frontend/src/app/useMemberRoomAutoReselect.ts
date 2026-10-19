import { useEffect, useRef } from "react";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import { remoteSessionTransport, sessionHostOrigin } from "../lib/remote/remoteWorkspace";
import {
  MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE,
  tryClaimMemberRoomAutoReselect,
  tryClaimMemberRoomAutoReselectRetry,
} from "../lib/central/memberRoomAutoReselectBudget";
import {
  isMembershipEndedError,
  isMembershipEndedTerminal,
  markMembershipEndedTerminal,
  membershipEndedTerminalScope,
  type MemberSelectError,
} from "../lib/memberMembershipEnded";
import { memberAdmissionFreeSelect } from "../lib/central/secureMemberEntry";
import { memberAdmissionServerFor } from "./remoteRoomAuthority";

/**
 * A cached background join payload is only reusable while its own bearer is
 * still alive. A clock-expired (or nearly expired) bearer must never be
 * re-installed: the install succeeds locally while the socket 401s, so the
 * loss heals "for free" without a real select and the hourly heal never
 * happens. Bearers from sessions that already failed authorization (kick/ban
 * revoke while inside the TTL) are poisoned the same way. Both cases fall
 * through to a fresh host select, where the host rechecks join/kick/ban and
 * mints a new bearer.
 */
const CACHED_JOIN_EXPIRY_MARGIN_MS = 60_000;
const authFailedSessionTokens = new Set<string>();

export function noteMemberRoomSessionAuthFailed(sessionToken: string): void {
  if (sessionToken) authFailedSessionTokens.add(sessionToken);
}

export function resetMemberRoomSessionAuthFailedForTests(): void {
  authFailedSessionTokens.clear();
}

export function isUsableCachedJoinPayload(payload: unknown, now: number = Date.now()): boolean {
  if (!payload || typeof payload !== "object") return false;
  const candidate = payload as { expires_at?: unknown; session_token?: unknown };
  if (typeof candidate.session_token === "string" && authFailedSessionTokens.has(candidate.session_token)) {
    return false;
  }
  if (typeof candidate.expires_at !== "string") return false;
  const expiresAt = Date.parse(candidate.expires_at);
  if (Number.isNaN(expiresAt)) return false;
  return expiresAt - now > CACHED_JOIN_EXPIRY_MARGIN_MS;
}

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
  setSelectError: (error: MemberSelectError | null | ((current: MemberSelectError | null) => MemberSelectError | null)) => void;
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
    // A kicked room is terminal: never auto re-select it again. The ended
    // view owns the acknowledgement; the error below only parks it here.
    if (isMembershipEndedTerminal(memberServerId, activeRoom.meetingId)) {
      const scope = membershipEndedTerminalScope(memberServerId, activeRoom.meetingId) ?? "kicked";
      (setSelectError as (update: MemberSelectError | ((current: MemberSelectError | null) => MemberSelectError | null)) => void)(
        (current) => current?.roomId === activeRoom.id && current.membershipEnded
          ? current
          : { roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE, membershipEnded: scope });
      return;
    }
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
        (setSelectError as (error: MemberSelectError) => void)(
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
        // A revoked or clock-expired cached bearer must never be re-installed
        // here either: fall through to the fresh select below, which spends
        // no budget on a channel-bound host and lets the host recheck
        // join/kick/ban.
        if (live && isUsableCachedJoinPayload(live)) {
          try {
            if (await acceptSession(live)) {
              (setSelectError as (update: (current: MemberSelectError | null) => MemberSelectError | null) => void)(
                (current) => (current?.roomId === activeRoom.id ? null : current));
              return;
            }
          } catch {
            // Fall through to a fresh select on the new admission.
          }
        }
        const payload = await selectRoom(memberServerId, activeRoom.meetingId);
        if (!await acceptSession(payload)) {
          (setSelectError as (error: MemberSelectError) => void)({ roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
        } else {
          (setSelectError as (update: (current: MemberSelectError | null) => MemberSelectError | null) => void)(
            (current) => (current?.roomId === activeRoom.id ? null : current));
        }
      } catch (error) {
        if (isMembershipEndedError(error)) {
          markMembershipEndedTerminal(memberServerId, activeRoom.meetingId);
          (setSelectError as (error: MemberSelectError) => void)(
            { roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE, membershipEnded: "kicked" });
        } else {
          (setSelectError as (error: MemberSelectError) => void)({ roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
        }
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
  // Host liveness comes from the admission transport itself, never from the
  // member host snapshot phase: the snapshot can report disconnected (or go
  // missing) while the adopted channel stays live, and the manual reconnect
  // still selects fine over it. Gating on the phase parks the heal exactly
  // when the channel is usable.
  const sessionLossKey = useRef("");
  const hadLiveSession = useRef(false);
  // Timestamp (Date.now) of the last automatic free heal per room. A free
  // heal that mints a bearer which dies almost immediately (kick, ban, or a
  // wedged channel) must not heal again automatically: without this the heal
  // re-selects on every loss edge and spins without spending budget on a
  // channel-bound host. No timers: the next loss edge compares timestamps.
  const lastFreeHealAt = useRef(new Map<string, number>());
  useEffect(() => {
    if (activeRoom.roomOrigin !== "remote_server" || !activeRoom.serverId) return;
    const memberServerId = memberAdmissionServerFor(activeRoom, ownerServerId, hasAnchor);
    if (!memberServerId) return;
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
    // A kicked room is terminal: park it without another select. The ended
    // view owns the acknowledgement; time passing alone never retries it.
    if (isMembershipEndedTerminal(memberServerId, activeRoom.meetingId)) {
      const scope = membershipEndedTerminalScope(memberServerId, activeRoom.meetingId) ?? "kicked";
      hadLiveSession.current = false;
      (setSelectError as (update: MemberSelectError | ((current: MemberSelectError | null) => MemberSelectError | null)) => void)(
        (current) => current?.roomId === activeRoom.id && current.membershipEnded
          ? current
          : { roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE, membershipEnded: scope });
      return;
    }
    // A session loss on a room that previously held a live session.
    let admission: unknown = null;
    try {
      admission = admissionFor(memberServerId);
    } catch {
      admission = null;
    }
    const transportActive =
      (admission as { transport?: { active?: boolean } })?.transport?.active === true;
    const free = memberAdmissionFreeSelect(admission);
    let skipped: string | null = null;
    if (isRefused(memberServerId)) skipped = "refused";
    else if (!admission) skipped = "no-admission";
    else if (!transportActive) skipped = "transport-inactive";
    else if (flight.current) skipped = "in-flight";
    else if (!free) {
      const claim = tryClaimMemberRoomAutoReselectRetry(accountId, memberServerId, activeRoom.meetingId);
      if (!claim.allowed) skipped = `retry-budget-${claim.reason}`;
    } else {
      // A free heal less than 60 s after the previous automatic heal of the
      // same room means the healed bearer died almost immediately: park with
      // the stop line instead of healing again. The user taps 다시 연결.
      const lastHeal = lastFreeHealAt.current.get(lossKey) ?? 0;
      if (Date.now() - lastHeal < 60_000) skipped = "healed-recently";
    }
    if (import.meta.env.DEV) console.debug("[member-select] session loss observed", {
      roomId: activeRoom.id, meetingId: activeRoom.meetingId, serverId: memberServerId,
      free, healed: skipped === null, ...(skipped ? { skipped } : {}),
    });
    if (skipped) {
      if (skipped === "retry-budget-exhausted" || skipped === "healed-recently") {
        (setSelectError as (error: MemberSelectError) => void)(
          { roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
      }
      if (skipped === "healed-recently") {
        // Park this loss permanently: a new live session re-arms the next
        // loss, but time passing alone never retries it.
        hadLiveSession.current = false;
      }
      return;
    }
    // Consume this loss before the async attempt so a refused room never
    // retries on the same loss; a new live session re-arms the next loss.
    hadLiveSession.current = false;
    if (free) lastFreeHealAt.current.set(lossKey, Date.now());
    flight.current = true;
    void (async () => {
      try {
        // The session-loss heal ALWAYS does a fresh host select: the host
        // rechecks join/kick/ban and mints a new bearer. A cached background
        // payload is never re-installed here — a revoked (kick/ban) bearer
        // would 401 and re-arm the loss into an uncapped spin on a
        // channel-bound host, and a clock-expired payload would "heal"
        // without a real select so the hourly heal never happens.
        const payload = await selectRoom(memberServerId, activeRoom.meetingId);
        if (!await acceptSession(payload)) {
          (setSelectError as (error: MemberSelectError) => void)({ roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
        } else {
          (setSelectError as (update: (current: MemberSelectError | null) => MemberSelectError | null) => void)(
            (current) => (current?.roomId === activeRoom.id ? null : current));
        }
      } catch (error) {
        // A refused fresh select means the host revoked this member
        // (kick/ban): poison the cached bearer so a later tap never
        // re-installs it either — the tap falls through to its own fresh
        // select and surfaces the refusal inline instead of 401-spinning.
        // A membership-ended refusal parks the room as terminal: the heal
        // and the auto re-select never retry it afterwards.
        try {
          const stale = liveSessionFor(memberServerId, activeRoom.meetingId) as {
            session_token?: unknown;
          } | null;
          if (stale && typeof stale.session_token === "string") {
            noteMemberRoomSessionAuthFailed(stale.session_token);
          }
        } catch {
          // Cache poisoning is best-effort; the stop line below already parks.
        }
        if (isMembershipEndedError(error)) {
          markMembershipEndedTerminal(memberServerId, activeRoom.meetingId);
          hadLiveSession.current = false;
          (setSelectError as (error: MemberSelectError) => void)(
            { roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE, membershipEnded: "kicked" });
        } else {
          (setSelectError as (error: MemberSelectError) => void)({ roomId: activeRoom.id, message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE });
        }
      } finally {
        flight.current = false;
      }
    })();
  }, [activeRoom, ownerServerId, accountId, hasAnchor, isRefused, admissionFor,
    guestSession, guestExpired, admittedSessionToken, liveSessionFor,
    selectRoom, acceptSession, setSelectError]);

  return { markAdmissionSeen: markSeen.current };
}

/**
 * Manual rail tap for a member room: prefers a live usable background
 * session (fresh bearer, never authorization-failed), otherwise selects once
 * through the host admission. Records the short
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
  setSelectError: (error: MemberSelectError | null | ((current: MemberSelectError | null) => MemberSelectError | null)) => void;
}): Promise<void> {
  try {
    const live = liveSessionFor(memberServerId, meetingId);
    let opened = false;
    // A revoked or clock-expired cached bearer must never be re-installed:
    // fall through to the fresh select below so the host rechecks membership.
    if (!isMembershipEndedTerminal(memberServerId, meetingId) && isUsableCachedJoinPayload(live)) {
      try {
        opened = await acceptSession(live);
      } catch {
        opened = false;
      }
    }
    if (opened) {
      (setSelectError as (update: (current: MemberSelectError | null) => MemberSelectError | null) => void)(
        (current) => (current?.roomId === roomId ? null : current));
    } else {
      const payload = await selectRoom(memberServerId, meetingId);
      if (import.meta.env.DEV) console.debug("[member-select] admission complete", {
        roomId, meetingId: payload.meeting_id, serverId: payload.server_id,
      });
      if (!await acceptSession(payload)) {
        (setSelectError as (error: MemberSelectError) => void)({ roomId, message: "방을 열지 못했어요. 다시 시도해 주세요." });
      } else {
        (setSelectError as (update: (current: MemberSelectError | null) => MemberSelectError | null) => void)(
          (current) => (current?.roomId === roomId ? null : current));
      }
    }
  } catch (error) {
    if (import.meta.env.DEV) console.debug("[member-select] select failed", {
      roomId, meetingId, serverId: memberServerId,
      message: error instanceof Error ? error.message : String(error),
    });
    if (isMembershipEndedError(error)) {
      markMembershipEndedTerminal(memberServerId, meetingId);
      (setSelectError as (error: MemberSelectError) => void)(
        { roomId, message: "방을 열지 못했어요. 다시 시도해 주세요.", membershipEnded: "kicked" });
    } else {
      (setSelectError as (error: MemberSelectError) => void)({ roomId, message: "방을 열지 못했어요. 다시 시도해 주세요." });
    }
  }
}
