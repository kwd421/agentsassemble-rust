import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CentralDirectoryState } from "./useCentralDirectory";
import { attachMemberHost, detachMemberHost, MemberAnchorMismatchError } from "../lib/central/memberAutoAttach";
import { MEMBER_ANCHORS_CHANGED_EVENT, loadMemberAnchor, loadMemberAnchors, removeMemberAnchor, removeMemberAnchorRoom, saveMemberAnchor } from "../lib/central/memberAnchors";
import { takeMemberJoinHandoff } from "../lib/central/memberJoinHandoff";
import { memberHostLiveLossFastDelay, memberHostOfflineBudgetExhausted, memberHostOfflineParkMs, memberHostReconnectCause, memberHostReconnectDelay, memberHostRetryAfterMs, noteMemberHostOfflinePreview, type MemberHostReconnectCause } from "../lib/central/memberRoomLive";
import { noteMemberHostExternalReselectAttach } from "../lib/central/memberRoomAutoReselectBudget";
import { centralDailyQuotaNotice, centralDailyQuotaWaitMs } from "../lib/central/centralRequestGovernor";
import { remoteWorkspaceFor, closeRemoteWorkspace, unbindRemoteSessionsForServer } from "../lib/remote/remoteWorkspace";
import { ApiError, userFacingError } from "../lib/apiErrors";
import type { SecureMemberAdmission } from "../lib/central/secureMemberEntry";
import type { HostRoomDirectory, ServerRoomDockSource } from "../lib/roomDockModel";
import type { RemoteHostConnection } from "../lib/serverConnectionState";

// One member-room select (including a single silent re-attach) never
// outlives this bound. A dead admission transport's multiplexed fetch has
// no timeout of its own, so without this a rail click would await forever
// with no error and the selection would never move.
const MEMBER_SELECT_TIMEOUT_MS = 30_000;

// After a day-long quota park the directory's own retry fires at the same
// reset instant. Host timers hold back past it so that retry lands first
// and reconcile supersedes them instead of doubling the central request.
// Deadline stagger alone cannot order them: coalesced timers (one big clock
// jump, sleep, a throttled tab) fire the host while the directory refresh
// is still in flight, so quota-parked hosts (tracked below) never attach
// straight from the anchor while central is unconfirmed — they hold back
// until the directory retry lands and reconcile follows it.
const QUOTA_RETRY_STAGGER_MS = 30_000;

function memberSelectErrorNeedsFreshChallenge(error: unknown): boolean {
  // Only a stale challenge (401) or a dead transport (no HTTP answer at
  // all) recovers with a fresh challenge/grant. Terminal answers from the
  // host (removed/kicked/revoked room, bad request) surface immediately
  // instead of destroying the live admission and spending another grant.
  if (error instanceof ApiError) {
    return error.status === 401 || error.status === 408 || error.status === 429 || error.status >= 500;
  }
  return true;
}

function memberSelectTimeout(): Error {
  return new Error("참가 서버의 응답이 없어요. 다시 시도해 주세요.");
}

export type MemberHostSnapshot = {
  serverId: string;
  phase: "connecting" | "connected" | "disconnected";
  origin: string;
  rooms: { room_id: string; name: string }[] | null;
  detail: string | null;
};

/**
 * Whether a member room is genuinely gone: only a live host that lists its
 * rooms without this one confirms a removal (friend removed/kicked/revoked
 * it). Every other state (re-attaching, disconnected, unknown host, unlisted
 * rooms) is transient and must retain the selection, never fall back.
 */
export function memberRoomRemovedWhileLive(
  room: { roomOrigin?: string; serverId?: string; meetingId: string },
  snapshots: MemberHostSnapshot[]
): boolean {
  if (room.roomOrigin !== "remote_server" || !room.serverId) return false;
  const host = snapshots.find((entry) => entry.serverId === room.serverId);
  return Boolean(host && host.phase === "connected" && host.rooms &&
    !host.rooms.some((known) => known.room_id === room.meetingId));
}

type DirectoryTuple = {
  epoch: string;
  fingerprint: string;
  origin: string;
  mode: string;
  status: string;
};

function tupleKey(tuple: DirectoryTuple): string {
  return [tuple.epoch, tuple.fingerprint, tuple.origin, tuple.mode, tuple.status].join("\0");
}

function toDockSources(rooms: { room_id: string; name: string }[] | null): ServerRoomDockSource[] | null {
  if (!rooms) return null;
  return rooms.map((room) => ({ room_id: room.room_id, label: room.name }));
}

/**
 * Automatic re-entry to every joined member host listed by the central
 * directory. First join keeps explicit consent (it writes the anchor);
 * afterwards the logged-in account re-attaches silently with a fresh grant
 * whenever the directory lists the membership. No timers or polling: attach
 * attempts run on directory/session/anchor changes and on explicit per-host
 * retry. Bearers stay memory-only; only the display anchor persists.
 */
export function useMemberHosts({
  personId,
  sessionToken,
  directory,
  onConfirmedMembershipLost,
}: {
  personId: string;
  sessionToken: string;
  directory: CentralDirectoryState | null;
  onConfirmedMembershipLost?: (serverId: string) => void;
}) {
  const [snapshots, setSnapshots] = useState<MemberHostSnapshot[]>([]);
  const [anchorRevision, setAnchorRevision] = useState(0);
  const flights = useRef(new Map<string, AbortController>());
  const admissions = useRef(new Map<string, SecureMemberAdmission>());
  const attempted = useRef(new Map<string, string>());
  const refused = useRef(new Set<string>());
  // Per-host automatic reconnect: bounded doubling backoff with jitter,
  // driven only by that host's own channel close, attach failure, or
  // explicit retry. No global loop, no central polling; each attempt
  // re-admits on demand. A 409 incarnation_conflict (endpoint revoked)
  // backs off to a slow cadence that wakes on real signals (directory
  // refresh, republish, user click); a 429 honors the server's Retry-After.
  // The attempt counter grows across timer-driven retries — only success
  // or an explicit user retry resets it.
  const reconnectTimers = useRef(new Map<string, number>());
  const reconnectAttempts = useRef(new Map<string, number>());
  const reconnectCauses = useRef(new Map<string, MemberHostReconnectCause>());
  const reconnectRetryAfter = useRef(new Map<string, number | null>());
  // Hosts whose reconnect timer was scheduled while the daily allowance was
  // spent. While central is unconfirmed such a timer never attaches on its
  // own: the directory owns the next attempt, so it holds back (timer only,
  // no central request) until the directory retry lands and reconcile
  // drives the single attach. Cleared together with the timer everywhere
  // the timer is superseded, dropped, or consumed.
  const quotaParkedHosts = useRef(new Set<string>());
  // Hosts whose live transport just dropped: the next automatic reconnect
  // (if any) is the one fast central origin lookup (memberHostLiveLossFastDelay)
  // instead of dropping straight onto the slow offline ladder. Consumed once
  // by scheduleReconnect; a genuine republish is usually adopted through the
  // directory tuple before it fires. A deliberately replaced/dropped
  // admission clears it with the timer, so it never fast-looks twice.
  const liveLossFastPending = useRef(new Set<string>());
  const clearReconnectTimer = useCallback((serverId: string) => {
    const timer = reconnectTimers.current.get(serverId);
    if (timer !== undefined) window.clearTimeout(timer);
    reconnectTimers.current.delete(serverId);
    quotaParkedHosts.current.delete(serverId);
    liveLossFastPending.current.delete(serverId);
  }, []);
  const snapshotsRef = useRef<MemberHostSnapshot[]>([]);
  snapshotsRef.current = snapshots;
  const directoryRef = useRef(directory);
  directoryRef.current = directory;
  const identityRef = useRef({ personId, sessionToken });
  identityRef.current = { personId, sessionToken };

  const setPhase = useCallback((serverId: string, phase: MemberHostSnapshot["phase"],
      rooms?: { room_id: string; name: string }[] | null, origin?: string, detail?: string | null) => {
    setSnapshots((current) => {
      const existing = current.find((entry) => entry.serverId === serverId);
      const next: MemberHostSnapshot = {
        serverId,
        phase,
        origin: origin ?? existing?.origin ?? "",
        rooms: rooms === undefined ? existing?.rooms ?? null : rooms,
        detail: detail === undefined ? existing?.detail ?? null : detail,
      };
      if (existing && existing.phase === next.phase && existing.rooms === next.rooms &&
          existing.origin === next.origin && existing.detail === next.detail) return current;
      return [...current.filter((entry) => entry.serverId !== serverId), next];
    });
  }, []);

  const dropHost = useCallback((serverId: string) => {
    clearReconnectTimer(serverId);
    flights.current.get(serverId)?.abort();
    flights.current.delete(serverId);
    const admission = admissions.current.get(serverId);
    admissions.current.delete(serverId);
    attempted.current.delete(serverId);
    detachMemberHost(admission);
    setSnapshots((current) => current.filter((entry) => entry.serverId !== serverId));
  }, [clearReconnectTimer]);

  const reattachHostRef = useRef<(serverId: string) => void>(() => undefined);
  // Hosts the person asked to reconnect (다시 연결): only that attempt may
  // spend central; automatic ones stay direct-only.
  const manualIntent = useRef(new Set<string>());
  const confirmedLossRef = useRef(onConfirmedMembershipLost);
  confirmedLossRef.current = onConfirmedMembershipLost;

  // Confirmed membership loss (kick/ban/revoke while central is connected):
  // the adopted secure channel and every bound room bearer close here instead
  // of staying open, and the caller drops the server's rooms from the rail.
  // A temporary central outage never reaches this: without a connected
  // directory listing, loss cannot be confirmed.
  const cleanupConfirmedLoss = useCallback((serverId: string) => {
    const admission = admissions.current.get(serverId);
    dropHost(serverId);
    // dropHost only closes an admit-only channel: an adopted room session
    // keeps its transport open, so close it outright, then drop the workspace
    // entry and every bearer bound to this host.
    try { admission?.transport.close(); } catch { /* transport close is best-effort */ }
    closeRemoteWorkspace(serverId);
    unbindRemoteSessionsForServer(serverId);
    confirmedLossRef.current?.(serverId);
  }, [dropHost]);

  const scheduleReconnect = useCallback((serverId: string) => {
    // An identity refusal blocks silent re-entry (fresh consent overwrites
    // the anchor); explicit retry past it stays manual.
    if (reconnectTimers.current.has(serverId) || refused.current.has(serverId)) return;
    const attempt = reconnectAttempts.current.get(serverId) ?? 0;
    const cause = reconnectCauses.current.get(serverId) ?? "transient";
    const retryAfterMs = reconnectRetryAfter.current.get(serverId) ?? null;
    // One fast origin lookup right after a confirmed live transport loss. It
    // is consumed once and does not advance the backoff ladder, so when it
    // still fails the existing slow ladder starts at its own first step
    // (daily preview budget and governor unchanged).
    const liveLossFast = liveLossFastPending.current.delete(serverId);
    if (!liveLossFast) reconnectAttempts.current.set(serverId, attempt + 1);
    // A spent daily allowance parks the host until the UTC-midnight reset:
    // the per-host backoff never fires into the quota block. The holdback
    // runs past the reset so the directory's own retry lands first. The same
    // park applies once the host's own offline-preview budget is spent (8
    // previews = 24 units = 10% of the daily allowance): further automatic
    // retries wait for midnight while an explicit 다시 연결 / room open
    // still tries once immediately.
    const activePerson = identityRef.current.personId;
    const quotaWait = centralDailyQuotaWaitMs(activePerson);
    const quotaParked = quotaWait > 0;
    const offlineParked = !quotaParked && cause === "offline" &&
      memberHostOfflineBudgetExhausted(activePerson, serverId);
    if (quotaParked || offlineParked) quotaParkedHosts.current.add(serverId);
    const delay = quotaParked
      ? quotaWait + QUOTA_RETRY_STAGGER_MS
      : offlineParked
        ? memberHostOfflineParkMs() + QUOTA_RETRY_STAGGER_MS
        : liveLossFast
          ? memberHostLiveLossFastDelay()
          : memberHostReconnectDelay(attempt, cause, retryAfterMs);
    const fire = () => {
      reconnectTimers.current.delete(serverId);
      // One in-flight reconnect per host, shared by every subscription: a
      // timer that fires into a running attempt stands down instead of
      // aborting it — that attempt's own outcome schedules the next step.
      if (flights.current.has(serverId)) return;
      // A quota-parked host never jumps the directory's reset retry. Its
      // deadline runs past the reset, but coalesced timers can still fire
      // it while the directory refresh is in flight; attaching straight
      // from the anchor then would double the central request when
      // reconcile follows the landed retry. Hold back (timer only, no
      // request) while central is unconfirmed instead. The same holdback
      // covers an offline-budget park: a timer that fires while the budget
      // is still spent re-parks instead of spending central.
      if (quotaParkedHosts.current.has(serverId) && directoryRef.current?.status === "central-unconfirmed") {
        reconnectTimers.current.set(serverId, window.setTimeout(fire, QUOTA_RETRY_STAGGER_MS));
        return;
      }
      if (memberHostOfflineBudgetExhausted(identityRef.current.personId, serverId) &&
        (reconnectCauses.current.get(serverId) ?? "transient") === "offline") {
        quotaParkedHosts.current.add(serverId);
        reconnectTimers.current.set(serverId, window.setTimeout(fire, memberHostOfflineParkMs() + QUOTA_RETRY_STAGGER_MS));
        return;
      }
      quotaParkedHosts.current.delete(serverId);
      reattachHostRef.current(serverId);
    };
    reconnectTimers.current.set(serverId, window.setTimeout(fire, delay));
  }, [clearReconnectTimer]);

  const attach = useCallback(async (serverId: string, epoch: string, origin: string, signal?: AbortSignal, options?: { internal?: boolean; central?: boolean }) => {
    const { personId: activePerson, sessionToken: activeToken } = identityRef.current;
    if (!activePerson || !activeToken || flights.current.has(serverId) || signal?.aborted) return;
    const controller = new AbortController();
    const onAbort = () => controller.abort();
    signal?.addEventListener("abort", onAbort, { once: true });
    flights.current.set(serverId, controller);
    const previousAnchor = loadMemberAnchor(activePerson, serverId);
    const central = options?.central ?? manualIntent.current.delete(serverId);
    setPhase(serverId, "connecting", previousAnchor?.rooms ?? null, origin, null);
    try {
      // The directory's current endpoint lets a restarted host be found at
      // its new address without central; the stored host key still pins it.
      const listed = directoryRef.current?.status === "connected"
        ? directoryRef.current.live?.servers.find((entry) => entry.server_id === serverId)?.endpoint ?? null
        : null;
      const endpoint = listed?.origin && listed.generation ? { origin: listed.origin, generation: listed.generation } : undefined;
      const attached = await attachMemberHost({ server_id: serverId, registration_epoch: epoch },
        activeToken, activePerson, controller.signal, undefined, { central, endpoint });
      // An aborted (StrictMode cleanup, identity flap, timed-out select)
      // flight must never land: it closes its admission instead of spending
      // another central request or clobbering a newer flight. Late completion
      // also never deletes a newer flight's bookkeeping.
      const ownFlight = flights.current.get(serverId) === controller;
      if (controller.signal.aborted || !ownFlight ||
          identityRef.current.personId !== activePerson ||
          identityRef.current.sessionToken !== activeToken) {
        detachMemberHost(attached.admission);
        if (ownFlight) flights.current.delete(serverId);
        return;
      }
      const current = identityRef.current;
      if (current.personId !== activePerson || current.sessionToken !== activeToken) {
        detachMemberHost(attached.admission);
        if (flights.current.get(serverId) === controller) flights.current.delete(serverId);
        return;
      }
      // While central is unconfirmed its membership list cannot confirm a
      // removal: the fresh admission stays with its own rooms. Only a
      // connected directory that no longer lists the membership drops it.
      const directory = directoryRef.current;
      const live = directory?.status === "connected" ? directory.live?.servers ?? null : null;
      if (live && !live.some((server) => server.server_id === serverId && server.relation === "member")) {
        // Confirmed loss mid-attach: the just-saved anchor goes too, or the
        // next retry would re-attach from it as if nothing happened.
        if (flights.current.get(serverId) === controller) flights.current.delete(serverId);
        detachMemberHost(attached.admission);
        if (activePerson) removeMemberAnchor(activePerson, serverId);
        cleanupConfirmedLoss(serverId);
        return;
      }
      if (flights.current.get(serverId) !== controller) {
        detachMemberHost(attached.admission);
        return;
      }
      flights.current.delete(serverId);
      const previousAdmission = admissions.current.get(serverId);
      const staleWorkspace = remoteWorkspaceFor(serverId);
      if (previousAdmission?.transport?.active) {
        // A live host channel already serves this host: the completing attach
        // is a redundant second channel (startup race, republish while live).
        // Never clobber the live admission with an admit-only duplicate that
        // the host reaps 30 s after open — that both spends the grant and
        // later orphans the registry the session-expiry heal needs. Close the
        // newcomer, keep serving on the live admission, and refresh the rail
        // from the fresh room list. No new admission, so the automatic
        // re-select generation stays untouched.
        if (import.meta.env.DEV) console.debug("[member-host] duplicate attach suppressed, keeping the live channel", {
          serverId,
          liveOrigin: (previousAdmission.transport as { hello?: { origin?: string } }).hello?.origin ?? null,
          freshOrigin: (attached.admission.transport as { hello?: { origin?: string } }).hello?.origin ?? null,
        });
        attached.admission.close();
        const timer = reconnectTimers.current.get(serverId);
        if (timer !== undefined) window.clearTimeout(timer);
        reconnectTimers.current.delete(serverId);
        quotaParkedHosts.current.delete(serverId);
        liveLossFastPending.current.delete(serverId);
        reconnectAttempts.current.delete(serverId);
        reconnectCauses.current.delete(serverId);
        reconnectRetryAfter.current.delete(serverId);
        const liveOrigin = (previousAdmission.transport as { hello?: { origin?: string } }).hello?.origin ||
          attached.anchor.endpoint_origin;
        setPhase(serverId, "connected", attached.rooms, liveOrigin, null);
        return;
      }
      previousAdmission?.close();
      admissions.current.set(serverId, attached.admission);
      // A re-attach replaces the host channel: sessions bound to the old
      // origin/channel must never outlive it. When the installed workspace
      // still points at a different (dead or superseded) transport, drop it
      // and every bearer bound to this host so the open room and background
      // subscriptions re-select fresh sessions on the new admission below
      // instead of looping on a refused stale bearer. Fake transports in
      // tests may omit hello: only drop when the workspace is provably
      // superseded (dead, or a different known origin).
      const staleOldOrigin = (staleWorkspace?.transport as { hello?: { origin?: string } } | undefined)?.hello?.origin;
      const freshOrigin = (attached.admission.transport as { hello?: { origin?: string } } | undefined)?.hello?.origin;
      if (staleWorkspace && staleWorkspace.transport !== attached.admission.transport &&
        (!staleWorkspace.transport.active ||
          (Boolean(staleOldOrigin) && Boolean(freshOrigin) && staleOldOrigin !== freshOrigin))) {
        if (import.meta.env.DEV) console.debug("[member-host] dropping stale workspace sessions", {
          serverId, oldOrigin: staleOldOrigin ?? null,
          newOrigin: freshOrigin ?? null,
        });
        closeRemoteWorkspace(serverId);
        unbindRemoteSessionsForServer(serverId);
      }
      attached.admission.transport.onClose(() => {
        if (identityRef.current.sessionToken !== activeToken) return;
        // A deliberate drop already removed the admission, so its close
        // never schedules a reconnect.
        if (!admissions.current.has(serverId)) return;
        setPhase(serverId, "disconnected");
        // The previously live host channel dropped: the stored origin may
        // already be the tunnel it just left, so the next automatic attempt
        // is one fast central origin lookup rather than the slow ladder.
        liveLossFastPending.current.add(serverId);
        scheduleReconnect(serverId);
      });
      const timer = reconnectTimers.current.get(serverId);
      if (timer !== undefined) window.clearTimeout(timer);
      reconnectTimers.current.delete(serverId);
      quotaParkedHosts.current.delete(serverId);
      liveLossFastPending.current.delete(serverId);
      reconnectAttempts.current.delete(serverId);
      reconnectCauses.current.delete(serverId);
      reconnectRetryAfter.current.delete(serverId);
      // Only an external re-attach (reconcile, timer-driven reattach,
      // explicit retry) releases one automatic room re-select for this host.
      // The failure path inside selectRoom re-attaches with internal: true
      // and never bumps this generation, so its fresh admission cannot
      // retrigger the automatic select into a grant-burning loop.
      if (!options?.internal && activePerson) {
        noteMemberHostExternalReselectAttach(activePerson, serverId);
      }
      setPhase(serverId, "connected", attached.rooms, attached.anchor.endpoint_origin, null);
    } catch (error) {
      const ownFlight = flights.current.get(serverId) === controller;
      if (ownFlight) flights.current.delete(serverId);
      if (controller.signal.aborted) return;
      if (!ownFlight) return;
      if (identityRef.current.personId !== activePerson || identityRef.current.sessionToken !== activeToken) return;
      if (error instanceof MemberAnchorMismatchError) {
        // Identity changed (epoch or host key): keep the known rooms
        // visible as disconnected with the Korean re-join reason. The
        // stale anchor is kept as the refusal baseline, so neither
        // auto-reconcile nor retry re-attaches silently; fresh consent
        // overwrites it (re-join path).
        refused.current.add(serverId);
        clearReconnectTimer(serverId);
        setPhase(serverId, "disconnected", previousAnchor?.rooms ?? null, origin, error.message);
        return;
      }
      const throttled = (error as { throttled?: unknown } | null)?.throttled === true;
      const rateLimited = throttled || (error as { status?: unknown } | null)?.status === 429;
      // A governor-throttled or central-429 failure surfaces as the room's
      // disconnected reason and retries only through the per-host backoff,
      // never immediately. While the daily allowance is spent the reason is
      // the reset-time line, whatever the failure kind.
      const quotaNotice = centralDailyQuotaNotice(activePerson);
      setPhase(serverId, "disconnected", loadMemberAnchor(activePerson, serverId)?.rooms ?? null, origin,
        quotaNotice ?? (rateLimited ? userFacingError(error, "다시 시도해 주세요.") : null));
      // The failure kind picks the backoff: a revoked endpoint (409
      // incarnation_conflict) rests on a slow cadence, a 429 honors the
      // server's Retry-After. The counter keeps growing across these
      // timer-driven retries; only success or explicit retry resets it. An
      // offline failure spends one unit of the host's daily preview budget
      // (each signed preview costs central even when refused).
      const cause = memberHostReconnectCause(error);
      reconnectCauses.current.set(serverId, cause);
      reconnectRetryAfter.current.set(serverId, memberHostRetryAfterMs(error));
      if (cause === "offline") noteMemberHostOfflinePreview(activePerson, serverId);
      scheduleReconnect(serverId);
    }
  }, [setPhase, dropHost, scheduleReconnect, clearReconnectTimer, cleanupConfirmedLoss]);

  // Account switch or logout ends every member session; anchors are cleared
  // by the central identity owner, so re-login re-attaches from membership.
  // The previous account's snapshots never stay visible for the next account.
  const identityKey = `${personId}\0${sessionToken}`;
  useEffect(() => {
    refused.current.clear();
    attempted.current.clear();
    setSnapshots((current) => (current.length === 0 ? current : []));
    return () => {
      for (const timer of reconnectTimers.current.values()) window.clearTimeout(timer);
      reconnectTimers.current.clear();
      quotaParkedHosts.current.clear();
      liveLossFastPending.current.clear();
      reconnectAttempts.current.clear();
      reconnectCauses.current.clear();
      reconnectRetryAfter.current.clear();
      for (const serverId of [...flights.current.keys()]) {
        flights.current.get(serverId)?.abort();
        flights.current.delete(serverId);
      }
      for (const serverId of [...admissions.current.keys()]) {
        detachMemberHost(admissions.current.get(serverId));
        admissions.current.delete(serverId);
      }
    };
  }, [identityKey]);

  useEffect(() => {
    const changed = () => setAnchorRevision((revision) => revision + 1);
    window.addEventListener(MEMBER_ANCHORS_CHANGED_EVENT, changed);
    return () => window.removeEventListener(MEMBER_ANCHORS_CHANGED_EVENT, changed);
  }, []);

  const reconcile = useCallback(() => {
    const { personId: activePerson } = identityRef.current;
    if (!activePerson) {
      if (import.meta.env.DEV) console.debug("[member-auto-attach] reconcile skipped: no signed-in person");
      return;
    }
    const live = directoryRef.current?.status === "connected" ? directoryRef.current.live?.servers ?? null : null;
    if (!live) {
      // Central is unconfirmed so its membership list cannot confirm or
      // deny anything: every joined host with a stored anchor still gets an
      // attach attempt scheduled through the existing anchor path in
      // reattachHost. A startup 429 must never strand the rail in silence;
      // a governor block inside the attempt ends in that host's scheduled
      // retry, and an identity refusal keeps blocking silently-retrying.
      const anchors = loadMemberAnchors(activePerson);
      if (import.meta.env.DEV) console.debug("[member-auto-attach] central unconfirmed, anchors drive attach", {
        anchors: anchors.map((anchor) => anchor.server_id),
      });
      for (const anchor of anchors) {
        if (anchor.hidden || refused.current.has(anchor.server_id)) continue;
        if (flights.current.has(anchor.server_id) || reconnectTimers.current.has(anchor.server_id)) continue;
        if (admissions.current.has(anchor.server_id)) continue;
        // An offline-budget park holds while central is unconfirmed too:
        // the parked timer (not a new attach) owns the next attempt.
        if (memberHostOfflineBudgetExhausted(activePerson, anchor.server_id) &&
          (reconnectCauses.current.get(anchor.server_id) ?? "transient") === "offline") continue;
        reattachHostRef.current(anchor.server_id);
      }
      return;
    }
    const members = live.filter((server) =>
      server.relation === "member" && Boolean(server.registration_epoch));
    const seen = new Set(members.map((server) => server.server_id));
    for (const snapshot of snapshotsRef.current) {
      if (!seen.has(snapshot.serverId)) {
        // Central connectedly confirms the membership is gone (kick/ban/
        // revoke): full loss cleanup, never a silent re-attach. A temporary
        // central outage never reaches here (unconfirmed directories return
        // above and keep every anchor).
        cleanupConfirmedLoss(snapshot.serverId);
        reconnectAttempts.current.delete(snapshot.serverId);
        reconnectCauses.current.delete(snapshot.serverId);
        reconnectRetryAfter.current.delete(snapshot.serverId);
        removeMemberAnchor(activePerson, snapshot.serverId);
      }
    }
    for (const server of members) {
      const epoch = server.registration_epoch || "";
      const tuple: DirectoryTuple = {
        epoch,
        fingerprint: server.host_key_fingerprint || "",
        origin: server.endpoint?.origin || "",
        mode: server.endpoint?.mode || "",
        status: server.endpoint?.status || "",
      };
      const anchor = loadMemberAnchor(activePerson, server.server_id);
      if (anchor?.hidden) {
        if (snapshotsRef.current.some((entry) => entry.serverId === server.server_id) ||
            flights.current.has(server.server_id)) dropHost(server.server_id);
        continue;
      }
      const origin = tuple.origin || anchor?.endpoint_origin || "";
      if (tuple.mode && tuple.mode !== "event_secure_v1") {
        attempted.current.set(server.server_id, tupleKey(tuple));
        setPhase(server.server_id, "disconnected", anchor?.rooms ?? null, origin, null);
        continue;
      }
      // A fresh invite join with explicit consent hands its live admission to
      // this registry. Only a handoff naming this exact person, server,
      // registration epoch and host key may lift the identity refusal: an
      // ordinary room select on a cached bearer never records one.
      const consented = takeMemberJoinHandoff(activePerson, server.server_id, {
        registration_epoch: epoch,
        host_key_fingerprint: tuple.fingerprint,
      });
      if (consented) {
        const live = consented.admission as SecureMemberAdmission | undefined;
        if (live?.transport?.active) {
          refused.current.delete(server.server_id);
          // The dead old-epoch entry shadowed the consented channel: drop it
          // so the registry serves the live admission the join produced.
          const stale = admissions.current.get(server.server_id);
          if (stale && stale !== live) {
            admissions.current.delete(server.server_id);
            detachMemberHost(stale);
          }
          admissions.current.set(server.server_id, live);
          clearReconnectTimer(server.server_id);
          reconnectAttempts.current.delete(server.server_id);
          reconnectCauses.current.delete(server.server_id);
          reconnectRetryAfter.current.delete(server.server_id);
          attempted.current.set(server.server_id, tupleKey(tuple));
          // The open room follows the new channel like any fresh admission.
          noteMemberHostExternalReselectAttach(activePerson, server.server_id);
          const consentedId = server.server_id;
          const consentedToken = identityRef.current.sessionToken;
          try {
            live.transport.onClose(() => {
              if (identityRef.current.sessionToken !== consentedToken) return;
              if (admissions.current.get(consentedId) !== live) return;
              setPhase(consentedId, "disconnected");
              liveLossFastPending.current.add(consentedId);
              scheduleReconnect(consentedId);
            });
          } catch { /* the close listener is best-effort */ }
          const consentedOrigin = (live.transport as { hello?: { origin?: string } }).hello?.origin || origin;
          setPhase(server.server_id, "connected", anchor?.rooms ?? null, consentedOrigin, null);
        }
        continue;
      }
      // No anchor but a live membership on a never-refused host: a fresh
      // device (first join happened elsewhere on the same account) attaches
      // silently. A recorded identity refusal still blocks this path so an
      // epoch/key change can never re-attach silently after the anchor is
      // gone; only fresh consent overwrites the anchor and lifts it.
      if (!anchor && refused.current.has(server.server_id)) {
        const known = snapshotsRef.current.find((entry) => entry.serverId === server.server_id);
        setPhase(server.server_id, "disconnected", known?.rooms ?? null, origin,
          known?.detail ?? "참가 서버 정보가 바뀌었어요. 새 초대로 다시 참가해 주세요.");
        continue;
      }
      if (flights.current.has(server.server_id) || refused.current.has(server.server_id)) {
        if (import.meta.env.DEV) console.debug("[member-auto-attach] skip: in-flight or refused", {
          serverId: server.server_id,
          flight: flights.current.has(server.server_id),
          refused: refused.current.has(server.server_id),
        });
        continue;
      }
      // A connected host with an unchanged identity and origin costs zero
      // central: directory status/mode flaps and focus-driven refreshes never
      // re-attach while the live channel already serves the listed origin.
      // The bookkeeping still advances to the current tuple so a later real
      // change (new origin/epoch) is the only thing that re-attaches.
      {
        const existing = snapshotsRef.current.find((entry) => entry.serverId === server.server_id);
        const admitted = admissions.current.get(server.server_id);
        const workspace = remoteWorkspaceFor(server.server_id);
        const liveChannel = Boolean(admitted?.transport.active) || Boolean(workspace?.transport.active);
        const identitySame = Boolean(anchor) && anchor!.registration_epoch === epoch &&
          (tuple.fingerprint === "" || anchor!.host_key_fingerprint === tuple.fingerprint);
        const originSame = Boolean(anchor) && (!tuple.origin || anchor!.endpoint_origin === tuple.origin);
        if (existing?.phase === "connected" && liveChannel && identitySame && originSame && anchor) {
          attempted.current.set(server.server_id, tupleKey(tuple));
          // The anchor follows the origin actually attached: a live channel
          // on a different origin than the stored anchor rewrites the anchor
          // locally (no central) so the disconnected screen never shows a
          // stale pre-restart origin.
          const actualOrigin = workspace?.transport.active
            ? (workspace.transport as { hello?: { origin?: string } }).hello?.origin || ""
            : (admitted?.transport as { hello?: { origin?: string } } | undefined)?.hello?.origin || "";
          if (actualOrigin && anchor.endpoint_origin !== actualOrigin) {
            try {
              saveMemberAnchor(activePerson, { ...anchor, endpoint_origin: actualOrigin, updated_at: Date.now() });
            } catch { /* anchor rewrite stays best-effort */ }
            setPhase(server.server_id, "connected", anchor.rooms ?? null, actualOrigin, null);
          } else if (existing.origin !== anchor.endpoint_origin) {
            setPhase(server.server_id, "connected", existing.rooms ?? null, anchor.endpoint_origin, null);
          }
          continue;
        }
      }
      if (attempted.current.get(server.server_id) === tupleKey(tuple)) {
        if (import.meta.env.DEV) console.debug("[member-auto-attach] skip: tuple already attempted", {
          serverId: server.server_id, origin: tuple.origin,
        });
        continue;
      }
      // Identity-only pin: epoch or host key fingerprint mismatch refuses.
      // Endpoint origin/generation comes fresh from the directory every
      // time; a new random public origin re-attaches silently. The stale
      // anchor is kept so the known rooms stay visible as disconnected
      // with the re-join reason until fresh consent overwrites it.
      if (anchor && (anchor.registration_epoch !== epoch ||
          (tuple.fingerprint !== "" && anchor.host_key_fingerprint !== tuple.fingerprint))) {
        // Identity changed (epoch or host key): the old admission's channel
        // is dead to the host, so detach it and drop it from the registry —
        // otherwise a later reconnect keeps selecting (or the TTL heal keeps
        // resolving admissionFor) on the stale entry instead of the live
        // workspace the re-join installs.
        const stale = admissions.current.get(server.server_id);
        admissions.current.delete(server.server_id);
        detachMemberHost(stale);
        refused.current.add(server.server_id);
        attempted.current.set(server.server_id, tupleKey(tuple));
        setPhase(server.server_id, "disconnected", anchor.rooms ?? null, origin,
          "참가 서버 정보가 바뀌었어요. 새 초대로 다시 참가해 주세요.");
        continue;
      }
      // A fresh consent-written anchor lifts a previous refusal.
      if (anchor && anchor.registration_epoch === epoch &&
          (tuple.fingerprint === "" || anchor.host_key_fingerprint === tuple.fingerprint)) {
        refused.current.delete(server.server_id);
      }
      // An offline-budget park survives even a fresh directory tuple: the
      // host stays parked until midnight while an explicit 다시 연결 /
      // room open still tries once immediately.
      if (anchor && memberHostOfflineBudgetExhausted(activePerson, server.server_id) &&
        (reconnectCauses.current.get(server.server_id) ?? "transient") === "offline") {
        if (!reconnectTimers.current.has(server.server_id)) scheduleReconnect(server.server_id);
        attempted.current.set(server.server_id, tupleKey(tuple));
        continue;
      }
      attempted.current.set(server.server_id, tupleKey(tuple));
      // A directory tuple the client has never tried (a republished host's
      // fresh origin/generation) supersedes a pending slow timer: drop it so
      // the prompt attempt below is the single in-flight reconnect and no
      // stale timer fires a duplicate behind it.
      clearReconnectTimer(server.server_id);
      // Another in-place flow may already hold this host's live session
      // (first-join consent just completed): adopt its rooms without opening
      // a second transport.
      const liveWorkspace = remoteWorkspaceFor(server.server_id);
      if (liveWorkspace?.transport.active && anchor) {
        const workspaceOrigin = (liveWorkspace.transport as { hello?: { origin?: string } }).hello?.origin || "";
        // A live channel on a stale origin never pins the anchor there: fall
        // through to a fresh attach below so the anchor follows the
        // directory's current origin (the origin actually attached next).
        if (!tuple.origin || !workspaceOrigin || workspaceOrigin === tuple.origin) {
          const adoptedId = server.server_id;
          const actualOrigin = workspaceOrigin || anchor.endpoint_origin;
          if (anchor.endpoint_origin !== actualOrigin && actualOrigin) {
            try {
              saveMemberAnchor(activePerson, { ...anchor, endpoint_origin: actualOrigin, updated_at: Date.now() });
            } catch { /* anchor rewrite stays best-effort */ }
          }
          liveWorkspace.transport.onClose(() => {
            if (admissions.current.has(adoptedId)) return;
            setPhase(adoptedId, "disconnected");
            liveLossFastPending.current.add(adoptedId);
            scheduleReconnect(adoptedId);
          });
          setPhase(server.server_id, "connected", anchor.rooms, actualOrigin);
          continue;
        }
      }
      void attach(server.server_id, epoch, origin);
    }
  }, [attach, dropHost, scheduleReconnect, setPhase, clearReconnectTimer, cleanupConfirmedLoss]);

  useEffect(() => {
    if (!personId || !sessionToken) {
      setSnapshots((current) => (current.length === 0 ? current : []));
      return;
    }
    reconcile();
  }, [directory, anchorRevision, personId, sessionToken, reconcile]);

  // Timer-driven reattach: a real signal (channel close, attach failure)
  // already recorded the cause and grew the counter, so this preserves the
  // backoff instead of resetting it.
  const reattachHost = useCallback((serverId: string) => {
    const { personId: activePerson } = identityRef.current;
    const live = directoryRef.current?.status === "connected" ? directoryRef.current.live?.servers ?? null : null;
    const server = live?.find((entry) => entry.server_id === serverId && entry.relation === "member");
    // An identity refusal stays on its re-join message until fresh consent
    // overwrites the anchor; a timer never attaches past it blind.
    if (refused.current.has(serverId) && !server) return;
    const anchor = activePerson ? loadMemberAnchor(activePerson, serverId) : null;
    const epoch = server?.registration_epoch || anchor?.registration_epoch || "";
    if (!epoch) return;
    // A live workspace channel only pins the registry entry when the registry
    // admission IS that workspace transport (same object): the open room and
    // its sessions are bound to it, so dropping the entry would orphan the
    // live admission (admissionFor null while the room stays live, and the
    // session-expiry heal parks with no-admission), and a fresh attach would
    // only mint a redundant admit-only channel the host reaps 30 s later.
    // Reconcile only; the adopt path keeps the host connected on its live
    // channel with zero central. A dead channel still falls through to a
    // fresh attach below. When the registry disagrees with the live workspace
    // (a dead epoch-1 entry while the re-join installed epoch-2), the early
    // return must not fire either: the entry is dropped below and a fresh
    // attach re-registers a live admission so the heal can select on it.
    const liveWorkspace = remoteWorkspaceFor(serverId);
    const registered = admissions.current.get(serverId);
    if (liveWorkspace?.transport.active === true && registered?.transport === liveWorkspace.transport) {
      clearReconnectTimer(serverId);
      reconcile();
      return;
    }
    attempted.current.delete(serverId);
    // Explicit timer/refresh intent takes over the parked holdback: the
    // attempt below either succeeds or schedules its own next step.
    clearReconnectTimer(serverId);
    dropHost(serverId);
    if (server) {
      // Epoch-change custody: the registry held a dead admission while a
      // different workspace (the re-join) went live. Reconcile alone only
      // adopts rooms and leaves admissionFor empty, so the TTL heal would
      // park on a dead entry. Attach fresh to re-register a live admission
      // (explicit retry already lifted the refusal gate above when listed).
      if (liveWorkspace?.transport.active === true &&
          (!registered || registered.transport?.active !== true) &&
          !refused.current.has(serverId)) {
        void attach(serverId, epoch,
          anchor?.endpoint_origin || liveWorkspace.transport.hello.origin || "");
        return;
      }
      reconcile();
      return;
    }
    // Central connectedly confirms no membership: this is loss, not outage.
    // Reconcile owns the loss cleanup; a timer must never re-attach from the
    // anchor as if nothing happened. Only an unconfirmed central may attach
    // straight from the anchor instead of stranding the retry in silence.
    if (live) return;
    void attach(serverId, epoch, anchor?.endpoint_origin || "");
  }, [attach, dropHost, reconcile, clearReconnectTimer]);

  const retryHost = useCallback((serverId: string) => {
    // Explicit user retry is fresh intent: restart the backoff and lift a
    // recorded refusal so the attempt re-runs (an identity mismatch
    // re-refuses with its re-join reason instead of stranding the retry).
    // Without a listed server the refusal stands and the retry stays put.
    const live = directoryRef.current?.status === "connected" ? directoryRef.current.live?.servers ?? null : null;
    const listed = live?.some((entry) => entry.server_id === serverId && entry.relation === "member") ?? false;
    if (refused.current.has(serverId) && !listed) return;
    refused.current.delete(serverId);
    reconnectAttempts.current.delete(serverId);
    reconnectCauses.current.delete(serverId);
    reconnectRetryAfter.current.delete(serverId);
    manualIntent.current.add(serverId);
    reattachHost(serverId);
    // An attach this retry started read the intent synchronously; an early
    // exit (live channel, flight in progress, no epoch) must not leave it for
    // a later automatic attempt to spend central on.
    manualIntent.current.delete(serverId);
  }, [reattachHost]);
  reattachHostRef.current = reattachHost;

  const selectRoom = useCallback(async (serverId: string, roomId: string) => {
    if (import.meta.env.DEV) console.debug("[member-select] select start", { serverId, roomId });
    const timeout = new AbortController();
    const timer = window.setTimeout(() => timeout.abort(memberSelectTimeout()), MEMBER_SELECT_TIMEOUT_MS);
    const timedOut = () => {
      if (!timeout.signal.aborted) return null;
      const reason = timeout.signal.reason;
      return reason instanceof Error ? reason : memberSelectTimeout();
    };
    const liveServer = () => directoryRef.current?.status === "connected"
      ? directoryRef.current.live?.servers.find((entry) => entry.server_id === serverId) ?? null
      : null;
    // While central is unconfirmed the directory cannot supply the epoch:
    // the saved anchor (identity pin) carries it instead, so a rail click
    // never waits on a central refresh first.
    const directoryTuple = () => {
      const entry = liveServer();
      const { personId: activePerson } = identityRef.current;
      const anchor = activePerson ? loadMemberAnchor(activePerson, serverId) : null;
      const epoch = entry?.registration_epoch || anchor?.registration_epoch || "";
      if (!epoch) return null;
      return {
        epoch,
        fingerprint: entry?.host_key_fingerprint || anchor?.host_key_fingerprint || "",
        origin: entry?.endpoint?.origin || anchor?.endpoint_origin || "",
        mode: entry?.endpoint?.mode || "",
        status: entry?.endpoint?.status || "",
      };
    };
    const anchorOrigin = () => {
      const { personId: activePerson } = identityRef.current;
      return (activePerson ? loadMemberAnchor(activePerson, serverId)?.endpoint_origin : "") || "";
    };
    // A manual attach replaces the reconcile bookkeeping for the directory's
    // current tuple; otherwise the anchor save at the end of the attach
    // triggers reconcile into an immediate duplicate attach.
    const markAttempted = () => {
      const tuple = directoryTuple();
      if (!tuple) return;
      attempted.current.set(serverId, tupleKey(tuple));
    };
    const attachFresh = async () => {
      if (refused.current.has(serverId) && !liveServer()) {
        const known = snapshotsRef.current.find((entry) => entry.serverId === serverId);
        throw new Error(known?.detail ?? "참가 서버 정보가 바뀌었어요. 새 초대로 다시 참가해 주세요.");
      }
      const tuple = directoryTuple();
      if (!tuple) throw new Error("서버에 연결 중이에요. 연결이 끝나면 다시 시도해 주세요.");
      attempted.current.delete(serverId);
      // A re-attach keeps the host's last-known rooms in the rail: only the
      // stale flight/admission bookkeeping is reset, never the snapshot, so
      // the clicked room stays listed (connecting) across the re-attach.
      flights.current.get(serverId)?.abort();
      flights.current.delete(serverId);
      admissions.current.get(serverId)?.close();
      admissions.current.delete(serverId);
      // Internal re-attach for this tap only: it must not bump the external
      // host generation, or the fresh admission would retrigger the automatic
      // re-select into an unbounded grant-burning loop.
      await attach(serverId, tuple.epoch, anchorOrigin() || tuple.origin, timeout.signal, { internal: true, central: true });
      markAttempted();
      return admissions.current.get(serverId);
    };
    try {
      let admission = admissions.current.get(serverId);
      if (!admission || admission.transport.active === false) {
        // The host reaps an admit-only channel 30 s after open, so a cached
        // admission whose transport is already dead can never carry a select:
        // attach fresh first instead of spending the click on a dead channel.
        // A transport that dies mid-select still retries below.
        if (import.meta.env.DEV && admission) console.debug("[member-select] cached channel dead, attaching fresh", {
          serverId, roomId,
        });
        admission = await attachFresh();
        if (!admission) throw timedOut() ?? new Error("서버에 연결 중이에요. 연결이 끝나면 다시 시도해 주세요.");
      }
      if (import.meta.env.DEV) console.debug("[member-select] admission ready, selecting room", { serverId, roomId });
      try {
        const selected = await admission.select(roomId, timeout.signal);
        if (import.meta.env.DEV) console.debug("[member-select] room admitted", { serverId, roomId, meetingId: selected.meeting_id });
        return selected;
      } catch (error) {
        const expiredInner = timedOut();
        if (expiredInner) throw expiredInner;
        if (!memberSelectErrorNeedsFreshChallenge(error)) {
          // Terminal host answer (removed/kicked/revoked room): refresh the
          // host's room list so the dead room leaves the rail, then surface
          // the original error. No select retry: it would fail the same way.
          if (import.meta.env.DEV) console.debug("[member-select] select refused, refreshing host rooms", {
            serverId, roomId,
            status: error instanceof ApiError ? error.status : undefined,
            code: error instanceof ApiError ? error.code || undefined : undefined,
          });
          const refreshTuple = directoryTuple();
          if (identityRef.current.personId && refreshTuple) {
            try {
              attempted.current.delete(serverId);
              await attach(serverId, refreshTuple.epoch,
                anchorOrigin() || refreshTuple.origin, timeout.signal, { internal: true, central: true });
              markAttempted();
            } catch {
              // The refresh is best-effort; the original refusal below is
              // what the user must act on. The snapshot already reflects
              // the failed attach (disconnected + retry).
            }
          }
          throw error;
        }
        if (import.meta.env.DEV) console.debug("[member-select] select failed, re-attaching with a fresh challenge", {
          serverId, roomId,
          status: error instanceof ApiError ? error.status : undefined,
          code: error instanceof ApiError ? error.code || undefined : undefined,
        });
        if (!identityRef.current.personId) throw error;
        const retry = await attachFresh();
        if (!retry) throw timedOut() ?? error;
        const selected = await retry.select(roomId, timeout.signal);
        if (import.meta.env.DEV) console.debug("[member-select] room admitted after re-attach", { serverId, roomId, meetingId: selected.meeting_id });
        return selected;
      }
    } catch (error) {
      const expired = timedOut();
      if (expired) {
        // The select timed out: the channel is wedged even when the transport
        // still reports active, so it must be replaced. Close the stored
        // admission (closing its transport) and delete the registry entry
        // before scheduling the reconnect below — otherwise reattachHost
        // early-returns on the wedged channel and the reconnect never
        // attaches a fresh one. The open room re-selects over the fresh
        // channel (and the session-expiry heal with it); 다시 연결 and room
        // taps do the same. The live-entry protection stays only on the
        // non-timeout paths (duplicate attach suppression, reattach on a
        // healthy live channel).
        flights.current.get(serverId)?.abort();
        flights.current.delete(serverId);
        const stored = admissions.current.get(serverId);
        admissions.current.delete(serverId);
        if (stored) {
          try {
            stored.transport.close();
          } catch {
            // Transport close is best-effort; the entry is already dropped.
          }
        }
        setPhase(serverId, "disconnected");
        reconnectCauses.current.set(serverId, "transient");
        reconnectRetryAfter.current.set(serverId, null);
        scheduleReconnect(serverId);
        throw expired;
      }
      throw error;
    } finally {
      window.clearTimeout(timer);
    }
  }, [attach, dropHost, reconcile, scheduleReconnect]);

  const removeRoom = useCallback((serverId: string, roomId: string) => {
    const { personId: activePerson } = identityRef.current;
    if (activePerson) removeMemberAnchorRoom(activePerson, serverId, roomId);
    setSnapshots((current) => current.map((entry) => entry.serverId === serverId && entry.rooms
      ? { ...entry, rooms: entry.rooms.filter((room) => room.room_id !== roomId) }
      : entry));
  }, []);

  const hostDirs = useMemo<HostRoomDirectory[]>(() => snapshots.map((snapshot) => ({
    serverId: snapshot.serverId,
    serverOrigin: snapshot.origin,
    rooms: toDockSources(snapshot.rooms),
    state: snapshot.phase === "connected" ? "connected" : snapshot.phase === "connecting" ? "connecting" : "disconnected",
  })), [snapshots]);

  const hostStates = useMemo<RemoteHostConnection[]>(() => snapshots.map((snapshot) => ({
    serverId: snapshot.serverId,
    state: snapshot.phase === "connecting" ? "connecting" : snapshot.phase === "connected" ? "connected" : "disconnected",
  })), [snapshots]);

  const hasAnchor = useCallback((serverId: string) => {
    const { personId: activePerson } = identityRef.current;
    return Boolean(activePerson && loadMemberAnchor(activePerson, serverId));
  }, []);

  const isRefused = useCallback((serverId: string) => refused.current.has(serverId), []);

  // Live background subscriptions resolve the host admission behind this
  // accessor; it never opens a second transport.
  const admissionFor = useCallback(
    (serverId: string) => admissions.current.get(serverId) ?? null,
    []
  );

  return { snapshots, hostDirs, hostStates, hasAnchor, isRefused, retryHost, selectRoom, removeRoom, admissionFor };
}
