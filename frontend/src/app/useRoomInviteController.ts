import { PUBLIC_INGRESS_START_ERROR } from "../lib/publicIngressStatus";
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
          setCopyStatus(error instanceof Error ? error.message : "공개 초대 상태를 불러오지 못했어요.");
        }
      });
    return () => {
      if (ingressOperationIsCurrent(generation)) retireIngressOperation();
    };
  }, [localOperatorEligible, remote?.sessionToken, remote?.deviceToken, modal?.roomId]);

  async function refreshPublicInviteState(generation: number) {
    assertIngressOperation(generation);
    setPublicAccessQuery("checking");
    try {
      const status = await queryPublicInviteState(generation);
      assertIngressOperation(generation);
      setPublicAccessQuery("confirmed");
      return status;
    } catch (error) {
      if (ingressOperationIsCurrent(generation)) setPublicAccessQuery("unavailable");
      throw error;
    }
  }

  function retryPublicInviteState() {
    const generation = beginIngressOperation();
    setCopyStatus("");
    void refreshPublicInviteState(generation).catch(error => {
      if (ingressOperationIsCurrent(generation)) setCopyStatus(error instanceof Error ? error.message : "외부 접속 상태를 확인하지 못했어요.");
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
      if (status.public_url) return status;
      if (!("tunnel" in status) || !status.tunnel.available) throw new Error(PUBLIC_INGRESS_START_ERROR);
      assertIngressOperation(generation);
      setCopyStatus("외부 접속 주소를 준비하는 중...");
      status = await startPublicInviteTunnel(() => assertIngressOperation(generation));
      assertIngressOperation(generation);
      setPublicInviteStatus(status);
      if (status.public_url && "tunnel" in status && status.tunnel.phase === "running") return status;
      const readyStatus = await waitForTunnelReady(generation);
      if (readyStatus.public_url && "tunnel" in readyStatus && readyStatus.tunnel.phase === "running") return readyStatus;
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
    setCopyStatus("외부 접속 주소를 준비하는 중...");
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
      setCopyStatus(
        latest.public_url
          ? "서버가 공개됐어요. 이제 외부 초대 링크를 만들 수 있어요."
          : PUBLIC_INGRESS_START_ERROR
      );
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
    setCopyStatus("외부 접속을 닫는 중...");
    try {
      assertIngressOperation(generation);
      const status = await stopPublicInviteTunnel(() =>
        assertIngressOperation(generation)
      );
      assertIngressOperation(generation);
      setPublicInviteStatus(status);
      setCopyStatus("외부 접속을 닫았어요. 방은 이 컴퓨터에서 계속 작동해요.");
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) return;
      setCopyStatus(error instanceof Error ? error.message : "서버 비공개 전환 실패");
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
    setCopyStatus("보안 초대 링크 생성 중...");
    try {
      await createManagedHumanInviteForRoom({
        room,
        displayName: options.displayName ?? "Guest",
        inviteScope,
        maxUses: options.maxUses,
        ttlSeconds: options.ttlSeconds,
        startTunnelIfNeeded,
      });
      setCopyStatus("보안 초대 링크 생성됨");
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) return;
      setCopyStatus(error instanceof Error ? error.message : "보안 초대 링크 생성 실패");
    }
  }

  async function generatePairing(room: RoomDockItem) {
    if ((!localOperatorEligible && !remote) || pairingCreationRef.current) return;
    pairingCreationRef.current = true;
    setPairingCreating(true);
    const generation = beginIngressOperation();
    setCopyStatus("기기 연결 링크를 만들고 있어요.");
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
      setCopyStatus("연결할 내 기기에서 링크를 열어 주세요.");
    } catch (error) {
      if (ingressOperationIsCurrent(generation)) {
        setCopyStatus(error instanceof Error ? error.message : "기기 연결 링크를 만들지 못했어요.");
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
