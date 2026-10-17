import { PUBLIC_INGRESS_START_ERROR } from "../lib/publicIngressStatus";
import { fetchLocalServerInfo } from "../lib/central/identity";
import { loadPublicIngressPreference, savePublicIngressPreference } from "../lib/publicIngressPreference";
import { fetchRemoteInviteOrigin, type RemoteInviteTransport } from "../api/roomInviteTransport";
import { copyText } from "../lib/copyInviteText";
import { useEffect, useRef, useState } from "react";
import {
  createManagedHumanInvite,
  fetchPublicInviteStatus,
  startPublicInviteTunnel,
  stopPublicInviteTunnel,
  type ManagedHumanInviteCustody,
  type PublicInviteStatus,
} from "../api";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { RoomAppearance } from "../lib/roomAppearance";
import type { DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import {
  sameManagerAuthority,
  useManagedHumanInvites,
} from "./useManagedHumanInvites";
import { createOperatorPairing } from "../api/operatorPairing";
import { useManagedAiInvites } from "./useManagedAiInvites";
import { useManagedOperatorPairings } from "./useManagedOperatorPairings";
import { userFacingError } from "../lib/apiErrors";

type InviteModalState = { roomId: string } | null;

export type PublicAccessTransition = "idle" | "starting" | "stopping";
export type PublicAccessQuery = "checking" | "confirmed" | "unavailable";

export type HumanInviteOptions = {
  maxUses: number;
  ttlSeconds: number;
  displayName?: string;
};

type UseRoomInviteControllerOptions = {
  remote?: RemoteInviteTransport;
  localOperatorEligible: boolean;
  resolveManagerRoomAuthority: (roomDockId: string) => DesktopManagerRoomAuthority;
};

const RETIRED_INGRESS_OPERATION = Symbol("retired ingress operation");

export function useRoomInviteController({
  localOperatorEligible,
  remote,
  resolveManagerRoomAuthority,
}: UseRoomInviteControllerOptions) {
  const [modal, setModal] = useState<InviteModalState>(null);
  const [copyStatus, setCopyStatus] = useState("");
  const [publicInviteStatus, setPublicInviteStatus] = useState<PublicInviteStatus | { public_url: string; remote: true } | null>(null);
  const [publicAccessQuery, setPublicAccessQuery] = useState<PublicAccessQuery>("checking");
  const [centralQuotaParkedUntil, setCentralQuotaParkedUntil] = useState<number | null>(null);
  const [publicAccessTransition, setPublicAccessTransition] =
    useState<PublicAccessTransition>("idle");
  const ingressGenerationRef = useRef(0);
  const ingressWaitRef = useRef<(() => void) | null>(null);
  const pairingCreationRef = useRef(false);
  const [pairingCreating, setPairingCreating] = useState(false);

  const managedHumanInvites = useManagedHumanInvites({
    remote,
    modalRoomDockId: modal?.roomId || "",
    currentPublicOrigin: publicInviteStatus?.public_url || "",
    resolveManagerRoomAuthority,
    copyText,
    captureCurrentPublicOriginRefresh,
    publishStatus: setCopyStatus,
  });
  const managedPairings = useManagedOperatorPairings({
    remote,
    roomDockId: modal?.roomId || "",
    publicOrigin: publicInviteStatus?.public_url || "",
    resolveManager: resolveManagerRoomAuthority,
    copyText,
    captureOriginRefresh: captureCurrentPublicOriginRefresh,
    publishStatus: setCopyStatus,
  });

  const connectorInvites = useManagedAiInvites({
    remote,
    roomDockId: modal?.roomId || "",
    publicOrigin: publicInviteStatus?.public_url || "",
    localOrigin: publicInviteStatus && "tunnel" in publicInviteStatus ? publicInviteStatus.tunnel.local_url : "",
    resolveManager: resolveManagerRoomAuthority,
    copyText,
    captureOriginRefresh: captureCurrentPublicOriginRefresh,
    publishStatus: setCopyStatus,
  });

  function resolveExactManagerAuthority(roomDockId: string) {
    try {
      return resolveManagerRoomAuthority(roomDockId);
    } catch {
      return null;
    }
  }

  function retireIngressOperation() {
    ingressGenerationRef.current += 1;
    const settle = ingressWaitRef.current;
    ingressWaitRef.current = null;
    settle?.();
  }

  function beginIngressOperation(
    nextState: PublicAccessTransition = "idle"
  ) {
    retireIngressOperation();
    setPublicAccessTransition(nextState);
    return ingressGenerationRef.current;
  }

  function ingressOperationIsCurrent(generation: number) {
    return ingressGenerationRef.current === generation;
  }

  function assertIngressOperation(generation: number) {
    if (!ingressOperationIsCurrent(generation)) {
      throw RETIRED_INGRESS_OPERATION;
    }
  }

  function waitForNextIngressPoll(generation: number) {
    assertIngressOperation(generation);
    return new Promise<void>((resolve) => {
      let settled = false;
      const timer = window.setTimeout(settle, 1000);
      function settle() {
        if (settled) return;
        settled = true;
        window.clearTimeout(timer);
        if (ingressWaitRef.current === settle) ingressWaitRef.current = null;
        resolve();
      }
      ingressWaitRef.current = settle;
    }).then(() => assertIngressOperation(generation));
  }

  function managerOperationIsCurrent(
    generation: number,
    roomDockId: string,
    authority: DesktopManagerRoomAuthority
  ) {
    return (
      ingressOperationIsCurrent(generation) &&
      sameManagerAuthority(resolveExactManagerAuthority(roomDockId), authority)
    );
  }

  function assertManagerOperation(
    generation: number,
    roomDockId: string,
    authority: DesktopManagerRoomAuthority
  ) {
    if (!managerOperationIsCurrent(generation, roomDockId, authority)) {
      throw RETIRED_INGRESS_OPERATION;
    }
  }

  function captureCurrentPublicOriginRefresh() {
    const generation = ingressGenerationRef.current;
    return async () => {
      try {
        const status = await refreshPublicInviteState(generation);
        if (!ingressOperationIsCurrent(generation)) return null;
        return Object.freeze({
          publicOrigin: status.public_url,
          localOrigin: "tunnel" in status ? status.tunnel.local_url : "",
          isCurrent: () => ingressOperationIsCurrent(generation),
        });
      } catch (error) {
        if (!ingressOperationIsCurrent(generation)) return null;
        throw error;
      }
    };
  }

  function open(roomId: string) {
    retireIngressOperation();
    setModal({ roomId });
    setCopyStatus("");
    setPublicInviteStatus(null);
    setCentralQuotaParkedUntil(null);
    setPublicAccessQuery("checking");
    setPublicAccessTransition("idle");
  }

  function close() {
    retireIngressOperation();
    setPublicAccessTransition("idle");
    setModal(null);
  }

  useEffect(() => () => retireIngressOperation(), []);

  useEffect(() => {
    if (!modal) return;
    if (!localOperatorEligible && !remote) {
      retireIngressOperation();
      setPublicAccessTransition("idle");
      setPublicInviteStatus(null);
      setPublicAccessQuery("unavailable");
      setCopyStatus("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있어요.");
      return;
    }
    const generation = beginIngressOperation();
    setCopyStatus("");
    refreshPublicInviteState(generation)
      .catch((error) => {
        if (ingressOperationIsCurrent(generation)) {
          setCopyStatus(userFacingError(error, "공개 초대 상태를 불러오지 못했어요."));
        }
      });
    return () => {
      if (ingressOperationIsCurrent(generation)) retireIngressOperation();
    };
  }, [localOperatorEligible, remote?.sessionToken, remote?.deviceToken, modal?.roomId]);

  // Owner decision (2026-10-17): external access remembers its last state.
  // If the owner left it open, reopen it at startup through the same
  // local-operator path the "외부 접속 열기" button uses (same server-side
  // authority check, no bypass). Closed stays closed; remote and other
  // non-operator surfaces never auto-open. A failed reopen stays quiet:
  // the hosting footer already shows the closed state.
  //
  // The local runtime (host surface, tunnel backend) is still starting at
  // first paint, so a single attempt can fail while nothing is wrong and
  // never run again. Retries therefore repeat the same owner start until it
  // succeeds, with an explicit budget: six waits (2 s, 5 s, 10 s, 30 s,
  // 60 s, 60 s — seven attempts total, only while the remembered choice is
  // still open). Each attempt is one status read plus at most one local
  // start. Retries stop on success, on an explicit close/stop (the
  // preference flips to closed), when this surface becomes ineligible, on
  // unmount, or when the budget runs out; a superseding ingress operation
  // (user pressed open/stop, modal refresh) retires a pending retry via its
  // generation. Cleanup owner: the effect cleanup below clears the timer.
  const AUTO_REOPEN_RETRY_DELAYS_MS = [2_000, 5_000, 10_000, 30_000, 60_000, 60_000];
  const autoReopenTimerRef = useRef<number | undefined>(undefined);
  const autoReopenDoneRef = useRef(false);
  useEffect(() => () => {
    if (autoReopenTimerRef.current !== undefined) {
      window.clearTimeout(autoReopenTimerRef.current);
      autoReopenTimerRef.current = undefined;
    }
  }, []);
  useEffect(() => {
    if (!localOperatorEligible || remote || autoReopenDoneRef.current) return;
    if (!loadPublicIngressPreference()) return;
    let cancelled = false;
    let attempt = 0;
    function scheduleRetry() {
      if (cancelled) return;
      if (attempt > AUTO_REOPEN_RETRY_DELAYS_MS.length) {
        autoReopenDoneRef.current = true;
        return;
      }
      const delay = AUTO_REOPEN_RETRY_DELAYS_MS[Math.min(attempt, AUTO_REOPEN_RETRY_DELAYS_MS.length - 1)];
      attempt += 1;
      autoReopenTimerRef.current = window.setTimeout(() => {
        autoReopenTimerRef.current = undefined;
        void runReopen();
      }, delay);
    }
    async function runReopen() {
      if (cancelled || autoReopenDoneRef.current) return;
      if (!loadPublicIngressPreference()) {
        autoReopenDoneRef.current = true;
        return;
      }
      const generation = beginIngressOperation();
      try {
        await preparePublicInvite(generation);
        if (!cancelled && ingressOperationIsCurrent(generation)) autoReopenDoneRef.current = true;
      } catch {
        if (cancelled || !ingressOperationIsCurrent(generation)) return;
        scheduleRetry();
      }
    }
    void runReopen();
    return () => {
      cancelled = true;
      if (autoReopenTimerRef.current !== undefined) {
        window.clearTimeout(autoReopenTimerRef.current);
        autoReopenTimerRef.current = undefined;
      }
    };
  }, [localOperatorEligible, remote?.sessionToken, remote?.deviceToken]);

  async function refreshPublicInviteState(generation: number) {
    assertIngressOperation(generation);
    setPublicAccessQuery("checking");
    try {
      const status = await queryPublicInviteState(generation);
      assertIngressOperation(generation);
      setPublicAccessQuery("confirmed");
      void refreshCentralQuotaState(generation);
      return status;
    } catch (error) {
      if (ingressOperationIsCurrent(generation)) setPublicAccessQuery("unavailable");
      throw error;
    }
  }

  // Reads the host's central publish park (`quota_parked_until` in
  // /api/server-info) so the footer stops claiming friends can connect while
  // the daily allowance is spent. Quiet: a failed read keeps tunnel-only state.
  async function refreshCentralQuotaState(generation: number) {
    if (remote || !localOperatorEligible) return;
    try {
      const info = await fetchLocalServerInfo();
      if (!ingressOperationIsCurrent(generation)) return;
      const parked = info.central_directory?.quota_parked_until ?? 0;
      setCentralQuotaParkedUntil(Number.isSafeInteger(parked) && parked > 0 ? parked : null);
    } catch {
      if (ingressOperationIsCurrent(generation)) setCentralQuotaParkedUntil(null);
    }
  }

  function retryPublicInviteState() {
    const generation = beginIngressOperation();
    setCopyStatus("");
    void refreshPublicInviteState(generation).catch(error => {
      if (ingressOperationIsCurrent(generation)) setCopyStatus(userFacingError(error, "외부 접속 상태를 확인하지 못했어요."));
    });
  }

  async function queryPublicInviteState(generation: number) {
    if (remote) {
      assertIngressOperation(generation);
      const status = await fetchRemoteInviteOrigin(remote, () => assertIngressOperation(generation));
      assertIngressOperation(generation);
      setPublicInviteStatus(status);
      return status;
    }
    if (!localOperatorEligible) {
      throw new Error("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있어요.");
    }
    assertIngressOperation(generation);
    const status = await fetchPublicInviteStatus(() =>
      assertIngressOperation(generation)
    );
    assertIngressOperation(generation);
    setPublicInviteStatus(status);
    return status;
  }

  async function waitForTunnelReady(generation: number) {
    for (let attempt = 0; attempt < 18; attempt += 1) {
      const nextStatus = await refreshPublicInviteState(generation);
      if (nextStatus.public_url && "tunnel" in nextStatus && nextStatus.tunnel.phase === "running") return nextStatus;
      if ("tunnel" in nextStatus && (nextStatus.tunnel.phase === "stopped" || nextStatus.tunnel.last_error)) return nextStatus;
      await waitForNextIngressPoll(generation);
    }
    return refreshPublicInviteState(generation);
  }

  async function preparePublicInvite(generation: number) {
    try {
      let status = await refreshPublicInviteState(generation);
      if (status.public_url) {
        // Already open on this host: re-affirm the remembered choice.
        if (!remote) savePublicIngressPreference(true);
        return status;
      }
      if (!("tunnel" in status) || !status.tunnel.available) throw new Error(PUBLIC_INGRESS_START_ERROR);
      assertIngressOperation(generation);
      status = await startPublicInviteTunnel(() => assertIngressOperation(generation));
      assertIngressOperation(generation);
      setPublicInviteStatus(status);
      if (status.public_url && "tunnel" in status && status.tunnel.phase === "running") {
        if (!remote) savePublicIngressPreference(true);
        return status;
      }
      const readyStatus = await waitForTunnelReady(generation);
      if (readyStatus.public_url && "tunnel" in readyStatus && readyStatus.tunnel.phase === "running") {
        if (!remote) savePublicIngressPreference(true);
        return readyStatus;
      }
      throw new Error(PUBLIC_INGRESS_START_ERROR);
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) throw error;
      assertIngressOperation(generation);
      throw new Error(PUBLIC_INGRESS_START_ERROR);
    }
  }

  async function requirePublicInviteReady(
    generation: number,
    startTunnelIfNeeded = false
  ) {
    const status = startTunnelIfNeeded
      ? await preparePublicInvite(generation)
      : await refreshPublicInviteState(generation);
    if (!status.public_url) {
      throw new Error("외부 접속을 먼저 열어 주세요.");
    }
    return status;
  }

  async function createManagedHumanInviteForRoom({
    room,
    displayName,
    inviteScope,
    ttlSeconds,
    maxUses,
    startTunnelIfNeeded,
  }: {
    room: RoomDockItem;
    displayName: string;
    inviteScope: RoomAppearance["inviteScope"];
    ttlSeconds: number;
    maxUses: number;
    startTunnelIfNeeded: boolean;
  }) {
    if (!localOperatorEligible && !remote) {
      throw new Error("현재 방의 초대 관리 권한을 확인할 수 없어요.");
    }
    const generation = beginIngressOperation();
    await requirePublicInviteReady(generation, startTunnelIfNeeded);
    const authority = resolveManagerRoomAuthority(room.id);
    assertManagerOperation(generation, room.id, authority);
    let custody: ManagedHumanInviteCustody;
    try {
      custody = await createManagedHumanInvite(
        {
          authority,
          displayName,
          inviteScope,
          ttlSeconds,
          maxUses,
        },
        () => assertManagerOperation(generation, room.id, authority), remote
      );
    } catch (error) {
      if (!managerOperationIsCurrent(generation, room.id, authority)) {
        throw RETIRED_INGRESS_OPERATION;
      }
      throw error;
    }
    const current = managerOperationIsCurrent(generation, room.id, custody.authority);
    const record = managedHumanInvites.retainAccepted({
      roomDockId: room.id,
      displayName,
      maxUses,
      ttlSeconds,
      operationGeneration: generation,
      current,
      custody,
    });
    if (record.retired) throw RETIRED_INGRESS_OPERATION;
    return record;
  }

  async function startTunnel() {
    if (!localOperatorEligible) {
      setCopyStatus("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있어요.");
      return;
    }
    const generation = beginIngressOperation("starting");
    try {
      assertIngressOperation(generation);
      const started = await startPublicInviteTunnel(() =>
        assertIngressOperation(generation)
      );
      assertIngressOperation(generation);
      setPublicInviteStatus(started);
      const latest =
        started.public_url && started.tunnel.phase === "running"
          ? started
          : await waitForTunnelReady(generation);
      assertIngressOperation(generation);
      // The hosting footer already shows "외부 접속 열림"; no success box.
      if (!latest.public_url) setCopyStatus(PUBLIC_INGRESS_START_ERROR);
      else if (!remote) savePublicIngressPreference(true);
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) return;
      setCopyStatus(PUBLIC_INGRESS_START_ERROR);
    } finally {
      if (ingressOperationIsCurrent(generation)) {
        setPublicAccessTransition("idle");
      }
    }
  }

  async function stopTunnel() {
    if (!localOperatorEligible) {
      setCopyStatus("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있어요.");
      return;
    }
    const generation = beginIngressOperation("stopping");
    try {
      assertIngressOperation(generation);
      const status = await stopPublicInviteTunnel(() =>
        assertIngressOperation(generation)
      );
      assertIngressOperation(generation);
      setPublicInviteStatus(status);
      if (!remote) savePublicIngressPreference(false);
      // The hosting footer already shows "외부 접속 꺼짐"; no success box.
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) return;
      setCopyStatus(userFacingError(error, "서버 비공개 전환 실패"));
    } finally {
      if (ingressOperationIsCurrent(generation)) {
        setPublicAccessTransition("idle");
      }
    }
  }

  async function generateSecureInvite(
    room: RoomDockItem,
    inviteScope: RoomAppearance["inviteScope"],
    options: HumanInviteOptions = { maxUses: 1, ttlSeconds: 86400 },
    startTunnelIfNeeded = false
  ) {
    try {
      await createManagedHumanInviteForRoom({
        room,
        displayName: options.displayName ?? "게스트",
        inviteScope,
        maxUses: options.maxUses,
        ttlSeconds: options.ttlSeconds,
        startTunnelIfNeeded,
      });
      // The new link row is the only feedback; no success box.
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) return;
      setCopyStatus(userFacingError(error, "보안 초대 링크 생성 실패"));
    }
  }

  async function generatePairing(room: RoomDockItem) {
    if ((!localOperatorEligible && !remote) || pairingCreationRef.current) return;
    pairingCreationRef.current = true;
    setPairingCreating(true);
    const generation = beginIngressOperation();
    try {
      const ready = await requirePublicInviteReady(generation);
      const authority = resolveManagerRoomAuthority(room.id);
      assertManagerOperation(generation, room.id, authority);
      const custody = await createOperatorPairing(authority,
        () => assertManagerOperation(generation, room.id, authority), remote);
      const current = managerOperationIsCurrent(generation, room.id, authority) &&
        custody.origin === ready.public_url;
      // Retain a confirmed but retired response so its grant can still be revoked.
      managedPairings.retain(custody, room.id, current);
      if (!current) return;
      // The new pairing row (with QR) is the only feedback; no success box.
    } catch (error) {
      if (ingressOperationIsCurrent(generation)) {
        setCopyStatus(userFacingError(error, "기기 연결 링크를 만들지 못했어요."));
      }
    } finally {
      pairingCreationRef.current = false;
      setPairingCreating(false);
    }
  }

  return {
    modal,
    copyStatus,
    humanInvites: managedHumanInvites.humanInvites,
    publicInviteStatus: publicInviteStatus && "tunnel" in publicInviteStatus ? publicInviteStatus : null,
    centralQuotaParkedUntil,
    publicAccessTransition,
    publicAccessQuery,
    retryPublicInviteState,
    invitePublicUrl:
      (publicInviteStatus && "stable_url" in publicInviteStatus ? publicInviteStatus.stable_url : "") || publicInviteStatus?.public_url || "",
    open,
    close,
    canControlIngress: localOperatorEligible,
    startTunnel,
    stopTunnel,
    generateSecureInvite,
    copyHumanInvite: managedHumanInvites.copy,
    revokeHumanInvite: managedHumanInvites.revoke,
    connectorInvites,
    attendeeInvites: { invites: connectorInvites.attendeeInvites, creating: connectorInvites.creating, create: (friendId: string) => void connectorInvites.create(friendId), copy: (key: string) => void connectorInvites.copy(key) },
    pairings: managedPairings.pairings,
    pairingCreating,
    generatePairing,
    copyPairing: managedPairings.copy,
    revokePairing: managedPairings.revoke,
  };
}
