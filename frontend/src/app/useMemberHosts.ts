import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CentralDirectoryState } from "./useCentralDirectory";
import { attachMemberHost, detachMemberHost, MemberAnchorMismatchError } from "../lib/central/memberAutoAttach";
import { MEMBER_ANCHORS_CHANGED_EVENT, loadMemberAnchor, removeMemberAnchor, removeMemberAnchorRoom } from "../lib/central/memberAnchors";
import { remoteWorkspaceFor } from "../lib/remote/remoteWorkspace";
import type { SecureMemberAdmission } from "../lib/central/secureMemberEntry";
import type { HostRoomDirectory, ServerRoomDockSource } from "../lib/roomDockModel";
import type { RemoteHostConnection } from "../lib/serverConnectionState";

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

  const attach = useCallback(async (serverId: string, epoch: string, origin: string) => {
    const { personId: activePerson, sessionToken: activeToken } = identityRef.current;
    if (!activePerson || !activeToken || flights.current.has(serverId)) return;
    const controller = new AbortController();
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
    let admission = admissions.current.get(serverId);
    if (!admission) {
      const live = directoryRef.current?.status === "connected" ? directoryRef.current.live?.servers ?? null : null;
      const epoch = live?.find((entry) => entry.server_id === serverId)?.registration_epoch;
      if (!epoch) throw new Error("서버에 연결 중이에요. 연결이 끝나면 다시 시도해 주세요.");
      attempted.current.delete(serverId);
      dropHost(serverId);
      await attach(serverId, epoch, loadMemberAnchor(identityRef.current.personId, serverId)?.endpoint_origin || "");
      admission = admissions.current.get(serverId);
      if (!admission) throw new Error("서버에 연결 중이에요. 연결이 끝나면 다시 시도해 주세요.");
    }
    try {
      return await admission.select(roomId);
    } catch (error) {
      const { personId: activePerson } = identityRef.current;
      if (!activePerson) throw error;
      const live = directoryRef.current?.status === "connected" ? directoryRef.current.live?.servers ?? null : null;
      const epoch = live?.find((entry) => entry.server_id === serverId)?.registration_epoch;
      if (!epoch) throw error;
      attempted.current.delete(serverId);
      dropHost(serverId);
      await attach(serverId, epoch, loadMemberAnchor(activePerson, serverId)?.endpoint_origin || "");
      const retry = admissions.current.get(serverId);
      if (!retry) throw error;
      return await retry.select(roomId);
    }
  }, [attach, dropHost]);

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
