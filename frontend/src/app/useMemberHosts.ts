import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CentralDirectoryState } from "./useCentralDirectory";
import { attachMemberHost, detachMemberHost, MemberAnchorMismatchError } from "../lib/central/memberAutoAttach";
import { MEMBER_ANCHORS_CHANGED_EVENT, loadMemberAnchor, removeMemberAnchor, removeMemberAnchorRoom } from "../lib/central/memberAnchors";
import { remoteWorkspaceFor } from "../lib/remote/remoteWorkspace";
import { ApiError } from "../lib/apiErrors";
import type { SecureMemberAdmission } from "../lib/central/secureMemberEntry";
import type { HostRoomDirectory, ServerRoomDockSource } from "../lib/roomDockModel";
import type { RemoteHostConnection } from "../lib/serverConnectionState";

// One member-room select (including a single silent re-attach) never
// outlives this bound. A dead admission transport's multiplexed fetch has
// no timeout of its own, so without this a rail click would await forever
// with no error and the selection would never move.
const MEMBER_SELECT_TIMEOUT_MS = 30_000;

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
}: {
  personId: string;
  sessionToken: string;
  directory: CentralDirectoryState | null;
}) {
  const [snapshots, setSnapshots] = useState<MemberHostSnapshot[]>([]);
  const [anchorRevision, setAnchorRevision] = useState(0);
  const flights = useRef(new Map<string, AbortController>());
  const admissions = useRef(new Map<string, SecureMemberAdmission>());
  const attempted = useRef(new Map<string, string>());
  const refused = useRef(new Set<string>());
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
    flights.current.get(serverId)?.abort();
    flights.current.delete(serverId);
    const admission = admissions.current.get(serverId);
    admissions.current.delete(serverId);
    attempted.current.delete(serverId);
    detachMemberHost(admission);
    setSnapshots((current) => current.filter((entry) => entry.serverId !== serverId));
  }, []);

  const attach = useCallback(async (serverId: string, epoch: string, origin: string, signal?: AbortSignal) => {
    const { personId: activePerson, sessionToken: activeToken } = identityRef.current;
    if (!activePerson || !activeToken || flights.current.has(serverId) || signal?.aborted) return;
    const controller = new AbortController();
    const onAbort = () => controller.abort();
    signal?.addEventListener("abort", onAbort, { once: true });
    flights.current.set(serverId, controller);
    const previousAnchor = loadMemberAnchor(activePerson, serverId);
    setPhase(serverId, "connecting", previousAnchor?.rooms ?? null, origin, null);
    try {
      const attached = await attachMemberHost({ server_id: serverId, registration_epoch: epoch },
        activeToken, activePerson, controller.signal);
      const current = identityRef.current;
      const live = directoryRef.current?.status === "connected" ? directoryRef.current.live?.servers ?? null : null;
      if (controller.signal.aborted || current.personId !== activePerson || current.sessionToken !== activeToken ||
          !live?.some((server) => server.server_id === serverId && server.relation === "member")) {
        detachMemberHost(attached.admission);
        if (current.personId === activePerson && current.sessionToken === activeToken) dropHost(serverId);
        return;
      }
      flights.current.delete(serverId);
      admissions.current.get(serverId)?.close();
      admissions.current.set(serverId, attached.admission);
      attached.admission.transport.onClose(() => {
        if (identityRef.current.sessionToken !== activeToken) return;
        setPhase(serverId, "disconnected");
      });
      setPhase(serverId, "connected", attached.rooms, attached.anchor.endpoint_origin, null);
    } catch (error) {
      flights.current.delete(serverId);
      if (controller.signal.aborted) return;
      if (identityRef.current.personId !== activePerson || identityRef.current.sessionToken !== activeToken) return;
      if (error instanceof MemberAnchorMismatchError) {
        // Identity changed (epoch or host key): keep the known rooms
        // visible as disconnected with the Korean re-join reason. The
        // stale anchor is kept as the refusal baseline, so neither
        // auto-reconcile nor retry re-attaches silently; fresh consent
        // overwrites it (re-join path).
        refused.current.add(serverId);
        setPhase(serverId, "disconnected", previousAnchor?.rooms ?? null, origin, error.message);
        return;
      }
      setPhase(serverId, "disconnected", loadMemberAnchor(activePerson, serverId)?.rooms ?? null, origin, null);
    }
  }, [setPhase, dropHost]);

  // Account switch or logout ends every member session; anchors are cleared
  // by the central identity owner, so re-login re-attaches from membership.
  const identityKey = `${personId}\0${sessionToken}`;
  useEffect(() => {
    refused.current.clear();
    attempted.current.clear();
    return () => {
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
    if (!activePerson) return;
    const live = directoryRef.current?.status === "connected" ? directoryRef.current.live?.servers ?? null : null;
    if (!live) return;
    const members = live.filter((server) =>
      server.relation === "member" && Boolean(server.registration_epoch));
    const seen = new Set(members.map((server) => server.server_id));
    for (const snapshot of snapshotsRef.current) {
      if (!seen.has(snapshot.serverId)) {
        dropHost(snapshot.serverId);
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
      if (flights.current.has(server.server_id) || refused.current.has(server.server_id)) continue;
      if (attempted.current.get(server.server_id) === tupleKey(tuple)) continue;
      // Identity-only pin: epoch or host key fingerprint mismatch refuses.
      // Endpoint origin/generation comes fresh from the directory every
      // time; a new random public origin re-attaches silently. The stale
      // anchor is kept so the known rooms stay visible as disconnected
      // with the re-join reason until fresh consent overwrites it.
      if (anchor && (anchor.registration_epoch !== epoch ||
          (tuple.fingerprint !== "" && anchor.host_key_fingerprint !== tuple.fingerprint))) {
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
      attempted.current.set(server.server_id, tupleKey(tuple));
      // Another in-place flow may already hold this host's live session
      // (first-join consent just completed): adopt its rooms without opening
      // a second transport.
      const liveWorkspace = remoteWorkspaceFor(server.server_id);
      if (liveWorkspace?.transport.active && anchor) {
        const adoptedId = server.server_id;
        liveWorkspace.transport.onClose(() => {
          if (admissions.current.has(adoptedId)) return;
          setPhase(adoptedId, "disconnected");
        });
        setPhase(server.server_id, "connected", anchor.rooms, anchor.endpoint_origin);
        continue;
      }
      void attach(server.server_id, epoch, origin);
    }
  }, [attach, dropHost, setPhase]);

  useEffect(() => {
    if (!personId || !sessionToken) {
      setSnapshots((current) => (current.length === 0 ? current : []));
      return;
    }
    reconcile();
  }, [directory, anchorRevision, personId, sessionToken, reconcile]);

  const retryHost = useCallback((serverId: string) => {
    const live = directoryRef.current?.status === "connected" ? directoryRef.current.live?.servers ?? null : null;
    const server = live?.find((entry) => entry.server_id === serverId && entry.relation === "member");
    if (!server?.registration_epoch) return;
    refused.current.delete(serverId);
    attempted.current.delete(serverId);
    dropHost(serverId);
    reconcile();
  }, [dropHost, reconcile]);

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
    // A manual attach replaces the reconcile bookkeeping for the directory's
    // current tuple; otherwise the anchor save at the end of the attach
    // triggers reconcile into an immediate duplicate attach.
    const markAttempted = () => {
      const entry = liveServer();
      const epoch = entry?.registration_epoch || "";
      if (!epoch) return;
      attempted.current.set(serverId, tupleKey({
        epoch,
        fingerprint: entry?.host_key_fingerprint || "",
        origin: entry?.endpoint?.origin || "",
        mode: entry?.endpoint?.mode || "",
        status: entry?.endpoint?.status || "",
      }));
    };
    const attachFresh = async () => {
      const entry = liveServer();
      const epoch = entry?.registration_epoch || "";
      if (!epoch) throw new Error("서버에 연결 중이에요. 연결이 끝나면 다시 시도해 주세요.");
      attempted.current.delete(serverId);
      dropHost(serverId);
      await attach(serverId, epoch, loadMemberAnchor(identityRef.current.personId, serverId)?.endpoint_origin || "", timeout.signal);
      markAttempted();
      return admissions.current.get(serverId);
    };
    try {
      let admission = admissions.current.get(serverId);
      if (!admission) {
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
          if (identityRef.current.personId && liveServer()?.registration_epoch) {
            try {
              attempted.current.delete(serverId);
              await attach(serverId, liveServer()?.registration_epoch || "",
                loadMemberAnchor(identityRef.current.personId, serverId)?.endpoint_origin || "", timeout.signal);
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
        // The transport is suspect (hung or dead): drop it and let a fresh
        // background attach restore the rooms as disconnected (anchor rooms
        // stay visible with retry) instead of stranding a hung select.
        dropHost(serverId);
        attempted.current.delete(serverId);
        reconcile();
        throw expired;
      }
      throw error;
    } finally {
      window.clearTimeout(timer);
    }
  }, [attach, dropHost, reconcile]);

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

  return { snapshots, hostDirs, hostStates, hasAnchor, isRefused, retryHost, selectRoom, removeRoom };
}
