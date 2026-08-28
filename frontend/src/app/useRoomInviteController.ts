import { useEffect, useRef, useState } from "react";
import {
  createManagedHumanInvite,
  createOperatorPairing,
  createRoomInvite,
  fetchPublicInviteStatus,
  startPublicInviteTunnel,
  stopPublicInviteTunnel,
  type ManagedHumanInviteCustody,
  type PublicInviteStatus,
  type RoomFriend,
} from "../api";
import {
  remoteClientPacketPreview,
  secureInviteCopyTarget,
} from "../lib/roomInviteCopy";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { RoomAppearance } from "../lib/roomAppearance";
import type { DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import {
  sameManagerAuthority,
  useManagedHumanInvites,
} from "./useManagedHumanInvites";

type InviteModalState = { roomId: string } | null;

type InviteRemoteClientPacketState = {
  friendName: string;
  preview: string;
};

export type PublicAccessTransition = "idle" | "starting" | "stopping";

export type HumanInviteOptions = {
  maxUses: number;
  ttlSeconds: number;
};

type UseRoomInviteControllerOptions = {
  localOperatorEligible: boolean;
  resolveManagerRoomAuthority: (roomDockId: string) => DesktopManagerRoomAuthority;
  sessionToken?: string;
};

const RETIRED_INGRESS_OPERATION = Symbol("retired ingress operation");

type PrepareClipboardDispatch = () => Promise<() => void>;

async function copyText(
  value: string,
  prepareDispatch?: PrepareClipboardDispatch
) {
  if (navigator.clipboard?.writeText) {
    const assertDispatch = await prepareDispatch?.();
    assertDispatch?.();
    try {
      await navigator.clipboard.writeText(value);
      return true;
    } catch {
      // Browser permission rejection may still permit the synchronous fallback.
    }
  }
  const assertDispatch = await prepareDispatch?.();
  assertDispatch?.();
  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  textarea.style.top = "0";
  textarea.style.opacity = "0";
  try {
    document.body.appendChild(textarea);
    textarea.focus({ preventScroll: true });
    textarea.select();
    textarea.setSelectionRange(0, value.length);
    assertDispatch?.();
    return document.execCommand("copy");
  } finally {
    textarea.remove();
  }
}

export function useRoomInviteController({
  localOperatorEligible,
  resolveManagerRoomAuthority,
  sessionToken = "",
}: UseRoomInviteControllerOptions) {
  const [modal, setModal] = useState<InviteModalState>(null);
  const [copyStatus, setCopyStatus] = useState("");
  const [agentInviteUrl, setAgentInviteUrl] = useState("");
  const [operatorPairingUrl, setOperatorPairingUrl] = useState("");
  const [publicInviteStatus, setPublicInviteStatus] = useState<PublicInviteStatus | null>(null);
  const [publicAccessTransition, setPublicAccessTransition] =
    useState<PublicAccessTransition>("idle");
  const [friendStatuses, setFriendStatuses] = useState<Record<string, string>>({});
  const [remoteClientPacket, setRemoteClientPacket] =
    useState<InviteRemoteClientPacketState>({ friendName: "", preview: "" });
  const ingressGenerationRef = useRef(0);
  const ingressWaitRef = useRef<(() => void) | null>(null);

  const managedHumanInvites = useManagedHumanInvites({
    modalRoomDockId: modal?.roomId || "",
    currentPublicOrigin: publicInviteStatus?.public_url || "",
    resolveManagerRoomAuthority,
    copyText,
    refreshCurrentPublicOrigin: refreshCurrentPublicOriginForCopy,
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

  async function refreshCurrentPublicOriginForCopy() {
    const generation = ingressGenerationRef.current;
    try {
      const status = await refreshPublicInviteState(generation);
      if (!ingressOperationIsCurrent(generation)) return null;
      return Object.freeze({
        publicOrigin: status.public_url,
        isCurrent: () => ingressOperationIsCurrent(generation),
      });
    } catch (error) {
      if (!ingressOperationIsCurrent(generation)) return null;
      throw error;
    }
  }

  function open(roomId: string) {
    retireIngressOperation();
    setModal({ roomId });
    setCopyStatus("");
    setAgentInviteUrl("");
    setOperatorPairingUrl("");
    setPublicInviteStatus(null);
    setPublicAccessTransition("idle");
    setFriendStatuses({});
    setRemoteClientPacket({ friendName: "", preview: "" });
  }

  function close() {
    retireIngressOperation();
    setPublicAccessTransition("idle");
    setModal(null);
  }

  useEffect(() => () => retireIngressOperation(), []);

  useEffect(() => {
    if (!modal) return;
    if (!localOperatorEligible) {
      retireIngressOperation();
      setPublicAccessTransition("idle");
      setPublicInviteStatus(null);
      setCopyStatus("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있습니다.");
      return;
    }
    const generation = beginIngressOperation();
    setCopyStatus("");
    refreshPublicInviteState(generation)
      .catch((error) => {
        if (ingressOperationIsCurrent(generation)) {
          setCopyStatus(error instanceof Error ? error.message : "공개 초대 상태를 불러오지 못했습니다.");
        }
      });
    return () => {
      if (ingressOperationIsCurrent(generation)) retireIngressOperation();
    };
  }, [localOperatorEligible, modal?.roomId]);

  async function refreshPublicInviteState(generation: number) {
    if (!localOperatorEligible) {
      throw new Error("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있습니다.");
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
      if (nextStatus.public_url && nextStatus.tunnel.phase === "running") return nextStatus;
      if (nextStatus.tunnel.phase === "stopped" || nextStatus.tunnel.last_error) return nextStatus;
      await waitForNextIngressPoll(generation);
    }
    return refreshPublicInviteState(generation);
  }

  async function preparePublicInvite(generation: number) {
    let status = await refreshPublicInviteState(generation);
    if (status.public_url) return status;
    if (!status.tunnel.available) {
      throw new Error("공개 URL을 만들 수 없습니다. cloudflared 설치 상태를 확인하세요.");
    }
    assertIngressOperation(generation);
    setCopyStatus("공개 터널 준비 중...");
    status = await startPublicInviteTunnel(() =>
      assertIngressOperation(generation)
    );
    assertIngressOperation(generation);
    setPublicInviteStatus(status);
    if (status.public_url && status.tunnel.phase === "running") return status;
    const readyStatus = await waitForTunnelReady(generation);
    if (readyStatus.public_url && readyStatus.tunnel.phase === "running") return readyStatus;
    throw new Error(
      readyStatus.tunnel.last_error ||
        "공개 터널이 아직 초대 URL을 보고하지 않았습니다. 잠시 후 다시 눌러 주세요."
    );
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
    if (!localOperatorEligible) {
      throw new Error("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있습니다.");
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
        () => assertManagerOperation(generation, room.id, authority)
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

  async function createSecureInviteForRoom({
    room,
    agentId,
    displayName,
    inviteScope,
    ttlSeconds = 86400,
    maxUses = 1,
    startTunnelIfNeeded = false,
  }: {
    room: RoomDockItem;
    agentId: string;
    displayName: string;
    inviteScope: RoomAppearance["inviteScope"];
    ttlSeconds?: number;
    maxUses?: number;
    startTunnelIfNeeded?: boolean;
  }) {
    if (!localOperatorEligible) {
      throw new Error("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있습니다.");
    }
    const generation = beginIngressOperation();
    await requirePublicInviteReady(generation, startTunnelIfNeeded);
    assertIngressOperation(generation);
    const invite = await createRoomInvite({
      meetingId: room.meetingId,
      agentId,
      displayName,
      inviteScope,
      ttlSeconds,
      maxUses,
      sessionToken,
    });
    assertIngressOperation(generation);
    const target = secureInviteCopyTarget({ joinUrl: invite.join_url || "" });
    if (!target.copyUrl) throw new Error(target.status);
    return { invite, target };
  }

  async function startTunnel() {
    if (!localOperatorEligible) {
      setCopyStatus("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있습니다.");
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
          ? "서버가 공개되었습니다. 이제 외부 초대 링크를 만들 수 있습니다."
          : latest.tunnel.last_error || "외부 접속 주소가 아직 준비되지 않았습니다."
      );
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) return;
      setCopyStatus(error instanceof Error ? error.message : "서버 공개 실패");
    } finally {
      if (ingressOperationIsCurrent(generation)) {
        setPublicAccessTransition("idle");
      }
    }
  }

  async function stopTunnel() {
    if (!localOperatorEligible) {
      setCopyStatus("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있습니다.");
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
      setAgentInviteUrl("");
      setOperatorPairingUrl("");
      setCopyStatus("외부 접속을 닫았습니다. 룸은 이 컴퓨터에서 계속 작동합니다.");
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
        displayName: "Guest",
        inviteScope,
        ...options,
        startTunnelIfNeeded,
      });
      setCopyStatus("보안 초대 링크 생성됨");
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) return;
      setCopyStatus(error instanceof Error ? error.message : "보안 초대 링크 생성 실패");
    }
  }

  async function generateAgentInvite(room: RoomDockItem, startTunnelIfNeeded = false) {
    setAgentInviteUrl("");
    setCopyStatus("외부 AI 세션 초대 링크 생성 중...");
    try {
      if (!localOperatorEligible) {
        throw new Error("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있습니다.");
      }
      const generation = beginIngressOperation();
      await requirePublicInviteReady(generation, startTunnelIfNeeded);
      assertIngressOperation(generation);
      const invite = await createRoomInvite({
        meetingId: room.meetingId,
        agentId: "external-agent",
        displayName: "External Agent",
        inviteScope: "room",
        ttlSeconds: 3600,
        clientType: "browser",
        providerKind: "manual",
        participantType: "agent",
        maxUses: 1,
        sessionToken,
      });
      assertIngressOperation(generation);
      const target = secureInviteCopyTarget({
        joinUrl: invite.join_url || "",
      });
      if (!target.copyUrl) throw new Error(target.status);
      setAgentInviteUrl(target.copyUrl);
      setCopyStatus("외부 AI 세션 1회용 초대 링크 생성됨");
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) return;
      setCopyStatus(error instanceof Error ? error.message : "외부 AI 세션 초대 생성 실패");
    }
  }

  async function copyAgentInvite() {
    if (!agentInviteUrl) return;
    const copied = await copyText(agentInviteUrl);
    setCopyStatus(copied ? "외부 AI 세션 초대 링크 복사됨" : "초대 링크 복사 실패");
  }

  async function generateOperatorPairing(room: RoomDockItem) {
    setCopyStatus("공개 주소용 운영자 연결 링크 생성 중...");
    try {
      if (!localOperatorEligible) {
        throw new Error("외부 접속 관리는 패키지 앱의 로컬 운영자만 사용할 수 있습니다.");
      }
      const generation = beginIngressOperation();
      await requirePublicInviteReady(generation, false);
      assertIngressOperation(generation);
      const pairing = await createOperatorPairing({
        meetingId: room.meetingId,
        sessionToken,
      });
      assertIngressOperation(generation);
      setOperatorPairingUrl(pairing.pairing_url);
      setCopyStatus("운영자 연결 링크 생성됨 · 2분 안에 한 번만 사용할 수 있습니다.");
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) return;
      setCopyStatus(error instanceof Error ? error.message : "운영자 연결 링크 생성 실패");
    }
  }

  async function copyOperatorPairing() {
    if (!operatorPairingUrl) return;
    const copied = await copyText(operatorPairingUrl);
    setCopyStatus(copied ? "운영자 연결 링크 복사됨" : "운영자 연결 링크 복사 실패");
  }

  async function copyRemoteClientPacket() {
    if (!remoteClientPacket.preview) return;
    setCopyStatus("");
    const copied = await copyText(remoteClientPacket.preview);
    setCopyStatus(copied ? "AI 입장 패킷 복사됨" : "패킷 복사 실패");
  }

  async function inviteFriend({
    friend,
    room,
    appearance,
    startTunnelIfNeeded = false,
  }: {
    friend: RoomFriend;
    room: RoomDockItem;
    appearance?: RoomAppearance;
    startTunnelIfNeeded?: boolean;
  }) {
    const friendId = friend.friend_id;
    setFriendStatuses((previous) => ({ ...previous, [friendId]: "초대 중" }));
    try {
      const isAiFriend = friend.participant_type !== "human";
      const inviteScope = appearance?.inviteScope || room.inviteScope || "room";
      if (!isAiFriend) {
        await createManagedHumanInviteForRoom({
          room,
          displayName: friend.display_name,
          inviteScope,
          ttlSeconds: 86400,
          maxUses: 1,
          startTunnelIfNeeded,
        });
        setRemoteClientPacket({ friendName: "", preview: "" });
        setFriendStatuses((previous) => ({
          ...previous,
          [friendId]: "초대 링크 생성됨",
        }));
        return;
      }
      const participantId = friend.source_agent_id || friend.friend_id;
      const { invite } = await createSecureInviteForRoom({
        room,
        agentId: participantId,
        displayName: friend.display_name,
        inviteScope,
        startTunnelIfNeeded,
      });
      const packetPreview = remoteClientPacketPreview(invite.remote_client_packet);
      setRemoteClientPacket({
        friendName: packetPreview ? friend.display_name : "",
        preview: packetPreview,
      });
      setFriendStatuses((previous) => ({
        ...previous,
        [friendId]: "입장 패킷 생성됨",
      }));
    } catch (error) {
      if (error === RETIRED_INGRESS_OPERATION) return;
      setFriendStatuses((previous) => ({
        ...previous,
        [friendId]:
          error instanceof Error
            ? error.message
            : "초대에 실패했습니다.",
      }));
    }
  }

  return {
    modal,
    copyStatus,
    secureInviteUrl: managedHumanInvites.secureInviteUrl,
    humanInvites: managedHumanInvites.humanInvites,
    agentInviteUrl,
    operatorPairingUrl,
    publicInviteStatus,
    publicAccessTransition,
    friendStatuses,
    remoteClientPacket,
    invitePublicUrl:
      publicInviteStatus?.stable_url || publicInviteStatus?.public_url || "",
    open,
    close,
    createSecureInviteForRoom,
    startTunnel,
    stopTunnel,
    generateSecureInvite,
    generateAgentInvite,
    generateOperatorPairing,
    copyAgentInvite,
    copyOperatorPairing,
    copySecureInvite: managedHumanInvites.copyCurrent,
    copyHumanInvite: managedHumanInvites.copy,
    revokeHumanInvite: managedHumanInvites.revoke,
    copyRemoteClientPacket,
    inviteFriend,
  };
}
