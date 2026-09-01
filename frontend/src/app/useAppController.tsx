import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  CSSProperties,
  MouseEvent as ReactMouseEvent,
} from "react";
import {
  createCompanionRoomInvite,
  refreshProviderCatalog,
  type ChannelNotificationSetting,
  type ChannelSettings,
  type RoomMember,
  type RoomSearchResult,
} from "../api";
import { useCanonicalRoom } from "../useCanonicalRoom";
import type {
  ChannelHeaderActions,
  ChannelSearchScope,
} from "../views/components/ChannelHeader";
import type { RoomMenuState } from "../views/components/RoomRail";
import { loadAgentActivityVisibility } from "../lib/agentActivityPreferences";
import { isDesktopWebview } from "../lib/desktopBridge";
import { consumeGuestRecoveryRequestFromUrl } from "../lib/guestRecovery";
import { roomAppearanceStyle } from "../lib/roomAppearance";
import {
  createStartupRoute,
  roomIsDisconnected,
  type RoomDockItem,
} from "../lib/roomDockModel";
import { consumeOperatorPairingTokenFromUrl } from "../lib/roomGuestSession";
import { roomPostingState } from "../lib/roomGuestPosting";
import { currentServerProductSurface } from "../lib/roomDirectoryContract";
import { remoteClientPacketPreview } from "../lib/roomInviteCopy";
import { roomRailMenuPosition } from "../lib/roomRailMenuPosition";
import {
  CHANNELS,
  EMPTY_ROOM,
  channelLastReadSummary,
  channelNotificationSummary,
  copyText,
  type Channel,
  type ChannelMenuState,
  type RoomSettingsSectionId,
  type RoomSettingsState,
} from "./appModel";
import { useMobilePanels } from "./useMobilePanels";
import { useAgentPresentation } from "./useAgentPresentation";
import { useAppMessageSearch } from "./useAppMessageSearch";
import { useDismissMenus } from "./useDismissMenus";
import { useRoomAdmission } from "./useRoomAdmission";
import { useRoomAppearanceAssets } from "./useRoomAppearanceAssets";
import { useRoomCreation } from "./useRoomCreation";
import { useRoomDirectory } from "./useRoomDirectory";
import { useRoomInviteController } from "./useRoomInviteController";
import { useRoomMembers } from "./useRoomMembers";
import {
  useRoomSettingsController,
  type RoomPreferenceAuthority,
} from "./useRoomSettingsController";
import { useSidebarResize } from "./useSidebarResize";

export function useAppController(deviceToken: string, clientId: string) {
  const [operatorPairingToken, setOperatorPairingToken] = useState(
    consumeOperatorPairingTokenFromUrl
  );
  const [guestRecoveryRequest, setGuestRecoveryRequest] = useState(
    consumeGuestRecoveryRequestFromUrl
  );
  const [startupRoute] = useState(() =>
    createStartupRoute({ operatorPairingPending: Boolean(operatorPairingToken) })
  );
  const [startupIdentityReady] = useState(isDesktopWebview);
  const guestInvite = startupRoute.guestInvite;
  const guestJoinToken = startupRoute.guestJoinToken;
  const startupIdentityResolved =
    startupIdentityReady ||
    Boolean(
      startupRoute.guestInvite ||
        startupRoute.guestSession ||
        startupRoute.guestJoinToken ||
        operatorPairingToken ||
        guestRecoveryRequest
    );
  // A built-in surface ("lobby") or an opaque custom channel id.
  const [channel, setChannel] = useState<string>(startupRoute.initialChannel);
  const [adminOpen, setAdminOpen] = useState(false);
  const [membersOpen, setMembersOpen] = useState(true);
  const startupHostEnabled =
    startupIdentityReady &&
    !startupRoute.guestInvite &&
    !startupRoute.guestSession &&
    !startupRoute.guestJoinToken &&
    !operatorPairingToken &&
    !guestRecoveryRequest;
  const {
    rooms,
    replaceRooms,
    markRoomRead: markRoomDirectoryRead,
    removeRoom,
    updateRoom,
    updateRoomByMeetingId,
    captureRoomDirectoryContinuity,
    validateRoomDirectoryContinuity,
    refreshRoomDirectory,
    verifyRoomDirectoryAuthority,
    managerAuthorityCurrent,
    resolveManagerRoomAuthority,
    syncIssue: roomDirectorySyncIssue,
  } = useRoomDirectory({
    initialRooms: startupRoute.startupRooms,
    hostEnabled: startupHostEnabled,
  });
  const hostServerProductSurface = useMemo(
    () => currentServerProductSurface(),
    [roomDirectorySyncIssue, startupIdentityResolved]
  );
  const [activeRoomId, setActiveRoomId] = useState(() => startupRoute.activeRoomId);
  const [roomMenu, setRoomMenu] = useState<RoomMenuState>(null);
  const [channelMenu, setChannelMenu] = useState<ChannelMenuState>(null);
  const [settingsModal, setSettingsModal] = useState<RoomSettingsState>(null);
  const [leaveRoomTargetId, setLeaveRoomTargetId] = useState("");
  const [agentCreateOpen, setAgentCreateOpen] = useState(false);
  const [guestAiPacketPreview, setGuestAiPacketPreview] = useState("");
  const [guestAiPacketStatus, setGuestAiPacketStatus] = useState("");
  const [agentActivityVisibility, setAgentActivityVisibility] = useState(
    loadAgentActivityVisibility
  );
  const [collapsedChannelSections, setCollapsedChannelSections] = useState<Record<string, boolean>>(
    {}
  );
  const [channelSearchQuery, setChannelSearchQuery] = useState("");
  const [messageSearchScope, setMessageSearchScope] = useState<ChannelSearchScope>("all");
  const [pendingMessageSearchTarget, setPendingMessageSearchTarget] = useState<{
    channelId: string;
    eventId: string;
  } | null>(null);
  const {
    mobileSidebarOpen,
    setMobileSidebarOpen,
    mobileRoomInfoOpen,
    setMobileRoomInfoOpen,
    mobileViewportIsActive,
    openMobileSidebar,
    closeMobileSidebar,
    openMobileRoomInfo,
    closeMobileRoomInfo,
    openMobileProfileFromPanel,
    closeMobileOverlays,
    handleMobileShellPointerDown,
    handleMobileShellPointerEnd,
    cancelMobileShellPointer,
  } = useMobilePanels({ canOpenRoomInfo: true });
  const {
    channelSidebarWidth,
    startSidebarResize,
    adjustSidebarWidthWithKeyboard,
  } = useSidebarResize();
  const onGuestRoomJoined = useCallback((room: RoomDockItem) => {
    replaceRooms([room]);
    setActiveRoomId(room.id);
    setChannel("lobby");
  }, [replaceRooms]);
  const onGuestAdmissionReset = useCallback(() => {
    setChannel("lobby");
    setGuestAiPacketPreview("");
    setGuestAiPacketStatus("");
  }, []);
  const clearOperatorPairingToken = useCallback(() => {
    setOperatorPairingToken("");
  }, []);
  const {
    guestSession,
    admittedSessionToken,
    guestExpired,
    guestJoinRequested,
    guestPreflightRetryable,
    guestJoinRetryable,
    pendingGuestDisplayName,
    pendingGuestAvatarImage,
    guestJoinStatus,
    guestAdmissionBusy,
    guestLocked,
    operatorPairingPending,
    operatorPairingState,
    guestReadOnly,
    guestPanelProfile,
    setPendingGuestDisplayName,
    setPendingGuestAvatarImage,
    requestGuestJoin,
    retryOperatorPairing,
    acceptRecoveredSession,
    expireGuestSession,
    clearGuestSession,
  } = useRoomAdmission({
    deviceToken,
    clientId,
    guestInvite,
    guestJoinToken,
    operatorPairingToken,
    onPairingTokenConsumed: clearOperatorPairingToken,
    initialSession: startupRoute.guestSession,
    onRoomJoined: onGuestRoomJoined,
    onResetToLobby: onGuestAdmissionReset,
  });
  const serverProductSurface =
    guestSession?.serverSurface.server_product_surface || hostServerProductSurface;
  const onRoomCreated = useCallback(
    (room: RoomDockItem) => {
      setActiveRoomId(room.id);
      setAdminOpen(false);
      setChannel("lobby");
      setRoomMenu(null);
      setChannelMenu(null);
      closeMobileOverlays();
    },
    [closeMobileOverlays]
  );
  const { addFreshRoom } = useRoomCreation({
    guestLocked,
    captureRoomDirectoryContinuity,
    validateRoomDirectoryContinuity,
    refreshRoomDirectory,
    verifyRoomDirectoryAuthority,
    onCreated: onRoomCreated,
  });
  const lobbyPostingState = useMemo(
    () =>
      roomPostingState({
        guestLocked,
        guestReadOnly,
        sessionToken: admittedSessionToken,
      }),
    [admittedSessionToken, guestLocked, guestReadOnly]
  );
  const activeRoom = rooms.find((room) => room.id === activeRoomId) ?? rooms[0] ?? EMPTY_ROOM;
  const activeRoomDisconnected = roomIsDisconnected(activeRoom);
  const activeOperationalMeetingId = activeRoomDisconnected ? "" : activeRoom.meetingId;
  // Rooms-as-server-objects: when a room becomes active, promote it to a
  // server-backed meeting (idempotent) so adding agents / roster / lobby always
  // have a real meeting to bind to instead of failing with "Meeting not found".
  const canonicalRoomAuth = activeRoomDisconnected
    ? undefined
    : guestLocked
    ? admittedSessionToken
      ? ({ kind: "session" as const, sessionToken: admittedSessionToken })
      : undefined
    : activeRoom.meetingId
      ? ({ kind: "host" as const, meetingId: activeRoom.meetingId })
      : undefined;
  const roomSurfaceReady = Boolean(
    serverProductSurface?.websocket_streams.includes("room_events")
  );
  const canonicalRoom = useCanonicalRoom({
    roomId: startupIdentityResolved && roomSurfaceReady ? activeOperationalMeetingId : "",
    auth: roomSurfaceReady ? canonicalRoomAuth : undefined,
    streams: serverProductSurface?.websocket_streams || [],
    serverSurface: serverProductSurface,
    viewerParticipantId: guestSession?.agentId || "operator-local",
    onUnauthorized: admittedSessionToken ? expireGuestSession : undefined,
  });
  const roomMembers = useRoomMembers({
    activeRoom,
    canonicalParticipants: canonicalRoom.participants,
    enabled: startupIdentityResolved && !activeRoomDisconnected,
  });
  const roomPreferenceAuthority: RoomPreferenceAuthority = guestLocked
    ? admittedSessionToken
      ? { kind: "remote", sessionToken: admittedSessionToken }
      : { kind: "remote-unavailable" }
    : { kind: "local", deviceToken };
  const activeRoomMembers = roomMembers.activeMembers;
  const roomSettings = useRoomSettingsController({
    activeRoom,
    preferenceAuthority: roomPreferenceAuthority,
    canonicalGlobalSettings: canonicalRoom.roomSettings,
    saveCanonicalGlobalSettings: canonicalRoom.sendRoomSettingsUpdate,
    onRoomMetadataLoaded: updateRoomByMeetingId,
    enabled: startupIdentityResolved && !activeRoomDisconnected,
  });
  const roomAppearanceAssets = useRoomAppearanceAssets({
    rooms, activeRoomId: activeRoom.id,
    activeRemoteRoomId: guestLocked ? activeRoom.id : "", remoteSessionToken: admittedSessionToken,
    canonicalAppearanceFor: roomSettings.appearanceFor,
    settingsStateFor: roomSettings.settingsStateFor,
    localAuthorityCurrent: managerAuthorityCurrent,
    resolveLocalManager: resolveManagerRoomAuthority,
    bindUploadedReference: (room, slot, url) => roomSettings.updateAppearance(room,
      slot === "banner" ? { bannerImage: url, bannerPreset: "custom" } : { iconImage: url }),
  });
  const roomAppearances = roomAppearanceAssets.appearances;
  const roomInvite = useRoomInviteController({
    localOperatorEligible: startupHostEnabled,
    resolveManagerRoomAuthority,
    sessionToken: admittedSessionToken,
  });
  const {
    modal: inviteModal,
    copyStatus: inviteCopyStatus,
    agentInviteUrl,
    operatorPairingUrl,
    publicInviteStatus,
    invitePublicUrl,
    open: openInviteModal,
    close: closeInviteModal,
    startTunnel: startInviteTunnel,
    stopTunnel: stopInviteTunnel,
    generateSecureInvite: generateInviteLink,
    generateAgentInvite: generateAgentInviteLink,
    generateOperatorPairing: generateOperatorPairingLink,
    copyAgentInvite: copyAgentInviteLink,
    copyOperatorPairing: copyOperatorPairingLink,
  } = roomInvite;
  const roomSocket = canonicalRoom.socket;
  const {
    activeRoomAgentSessions,
    activeRoomCapabilities,
    activeRoomHistory,
    visibleRoomTimelineEvents,
    loadCanonicalRoomHistory,
    sendAgentControl,
    sendAgentConfigure,
    sendParticipantMute,
    sendParticipantRole,
    loadProviderUsage,
    quotaViewer,
    scopedAgents,
    scopedViewerDisplayName,
    changeAgentActivityVisibility,
    scopedMentionables,
    scopedOnlineCount,
    typingIndicators,
  } = useAgentPresentation({
    canonicalRoom,
    activeRoom,
    activeRoomMembers,
    guestLocked,
    guestSession,
    agentActivityVisibility,
    setAgentActivityVisibility,
  });
  useDismissMenus(roomMenu, channelMenu, setRoomMenu, setChannelMenu);
  const activeChannelSettings = roomSettings.channelSettingsFor(activeRoom);
  const { roomHttpAuthority, roomMessageSearch } = useAppMessageSearch({
    roomId: activeOperationalMeetingId,
    scope: messageSearchScope,
    sessionToken: admittedSessionToken,
    localAvailable: !guestLocked && isDesktopWebview(),
  });
  const messageSearchChannelLabels = { lobby: "general" };
  useEffect(() => {
    setMessageSearchScope("all");
    setPendingMessageSearchTarget(null);
  }, [activeRoom.meetingId]);
  const menuRoom = roomMenu ? rooms.find((room) => room.id === roomMenu.roomId) : undefined;
  const menuChannel = channelMenu
    ? CHANNELS.find((item) => item.id === channelMenu.channelId)
    : undefined;
  const menuChannelDisplay = menuChannel;
  const activeChannelDisplay =
    CHANNELS.find((item) => item.id === channel) || CHANNELS[0];
  const visibleChannels = CHANNELS;
  const channelSearchNeedle = channelSearchQuery.trim().toLowerCase();

  function selectRoom(roomId: string) {
    setActiveRoomId(roomId);
    setAdminOpen(false);
    setChannel("lobby");
    setRoomMenu(null);
    setChannelMenu(null);
    closeMobileOverlays();
  }

  function openRoomMenu(event: ReactMouseEvent, room: RoomDockItem) {
    event.preventDefault();
    setActiveRoomId(room.id);
    setAdminOpen(false);
    const position = roomRailMenuPosition(
      { x: event.clientX, y: event.clientY },
      { width: window.innerWidth, height: window.innerHeight }
    );
    setRoomMenu({
      roomId: room.id,
      x: position.left,
      y: position.top,
    });
    setChannelMenu(null);
  }

  function openChannelMenu(event: ReactMouseEvent, channelId: Channel) {
    event.preventDefault();
    setRoomMenu(null);
    setChannelMenu({
      channelId,
      x: Math.min(event.clientX, window.innerWidth - 232),
      y: Math.min(event.clientY, window.innerHeight - 240),
    });
  }

  function markRoomRead(roomId: string) {
    const readAt = new Date().toISOString();
    markRoomDirectoryRead(roomId, readAt);
    setRoomMenu(null);
    setChannelMenu(null);
  }

  function inviteRoom(roomId: string) {
    setActiveRoomId(roomId);
    setChannel("lobby");
    setAdminOpen(false);
    closeMobileOverlays();
    openInviteModal(roomId);
    setRoomMenu(null);
    setChannelMenu(null);
  }

  function openAgentCreate() {
    setAgentCreateOpen(true);
    void refreshProviderCatalog(false).catch(() => {
      // The modal keeps the last verified catalog and exposes its loading/error state.
    });
    closeMobileOverlays();
    setRoomMenu(null);
    setChannelMenu(null);
  }

  function openRoomSettings(roomId: string, initialSectionId: RoomSettingsSectionId = "settings-overview") {
    if (guestLocked) return;
    setActiveRoomId(roomId);
    setAdminOpen(false);
    setSettingsModal({ roomId, initialSectionId });
    setRoomMenu(null);
    setChannelMenu(null);
  }

  function removeAcknowledgedRoom(roomId: string) {
    const remainingRooms = removeRoom(roomId);
    if (activeRoom.id === roomId) {
      setActiveRoomId(remainingRooms[0]?.id || "");
      setChannel("lobby");
      setAdminOpen(false);
    }
    setRoomMenu(null);
    setChannelMenu(null);
  }

  function exitGuestSurface() {
    clearGuestSession();
    const url = new URL(window.location.href);
    url.pathname = "/";
    url.search = "";
    url.hash = "";
    window.location.href = url.toString();
  }

  async function leaveRoom(roomId: string) {
    if (guestLocked && guestExpired && roomId === activeRoom.id) {
      removeAcknowledgedRoom(roomId);
      exitGuestSurface();
      return;
    }
    if (roomId !== activeRoom.id || !roomSocket?.ready()) {
      throw new Error("나갈 서버를 먼저 열고 연결이 완료될 때까지 기다려 주세요.");
    }
    await roomSocket.command("participant.leave", {});
    removeAcknowledgedRoom(roomId);
    if (guestLocked) {
      exitGuestSurface();
    }
  }

  function goToChannel(next: Channel) {
    setChannel(next);
    setAdminOpen(false);
    setChannelMenu(null);
    closeMobileOverlays();
  }

  function openCrossChannelSearchResult(result: RoomSearchResult) {
    const targetChannel = result.channel_id;
    if (targetChannel !== "lobby") {
      roomMessageSearch.setError("검색 결과의 채널을 더 이상 열 수 없습니다.");
      return;
    }
    setPendingMessageSearchTarget({
      channelId: targetChannel,
      eventId: result.event_id,
    });
    goToChannel(targetChannel);
  }

  async function createCompanionAiPacket() {
    if (!admittedSessionToken || !guestSession) return;
    setGuestAiPacketStatus("AI 입장 패킷 생성 중...");
    try {
      const invite = await createCompanionRoomInvite({
        sessionToken: admittedSessionToken,
        agentId: `${guestSession.agentId || "friend"}-ai`,
        displayName: `${guestSession.displayName || "Friend"} AI`,
      });
      const preview = remoteClientPacketPreview(invite.remote_client_packet);
      setGuestAiPacketPreview(preview);
      setGuestAiPacketStatus(preview ? "AI 입장 패킷 생성됨" : "AI 입장 패킷이 비어 있습니다");
    } catch {
      setGuestAiPacketStatus("AI 입장 패킷 생성 실패. 초대 세션 권한과 공개 URL 설정을 확인하세요.");
    }
  }

  async function copyGuestAiPacket() {
    if (!guestAiPacketPreview) return;
    const copied = await copyText(guestAiPacketPreview);
    setGuestAiPacketStatus(copied ? "AI 입장 패킷 복사됨" : "AI 입장 패킷 복사 실패");
  }

  const toggleMembers = useCallback(() => setMembersOpen((value) => !value), []);
  const showMembers = !adminOpen;
  const inviteModalRoom = inviteModal ? rooms.find((room) => room.id === inviteModal.roomId) : undefined;
  const settingsModalRoom = settingsModal
    ? rooms.find((room) => room.id === settingsModal.roomId)
    : undefined;
  const leaveRoomTarget = rooms.find((room) => room.id === leaveRoomTargetId);
  const settingsModalInitialSectionId = settingsModal?.initialSectionId;
  const inviteModalAppearance = inviteModalRoom
    ? roomSettings.appearanceFor(inviteModalRoom)
    : undefined;
  const activeAppearance = roomAppearanceAssets.appearanceFor(activeRoom);
  const activeRoomStyle = useMemo(() => roomAppearanceStyle(activeAppearance), [activeAppearance]);
  const shellStyle = useMemo(
    () =>
      ({
        ...activeRoomStyle,
        "--dc-sidebar-width": `${channelSidebarWidth}px`,
      }) as CSSProperties,
    [activeRoomStyle, channelSidebarWidth]
  );
  async function updateMemberRole(memberId: string, role: RoomMember["role"]) {
    await sendParticipantRole(memberId, role);
  }

  function updateChannelSetting(channelId: string, updates: Partial<ChannelSettings>) {
    void roomSettings.updateChannelSetting(activeRoom, channelId, updates).catch(() => undefined);
  }

  function markChannelRead(channelId: string, cursor = "") {
    updateChannelSetting(channelId, { lastReadAt: cursor || new Date().toISOString() });
    setChannelMenu(null);
  }

  function setChannelNotifications(
    channelId: Channel,
    notifications: ChannelNotificationSetting
  ) {
    updateChannelSetting(channelId, { notifications });
    setChannelMenu(null);
  }

  function channelHeaderActions(channelId: Channel): ChannelHeaderActions {
    const setting = activeChannelSettings[channelId];
    return {
      notificationSummary: channelNotificationSummary(setting),
      lastReadSummary: channelLastReadSummary(setting),
      lastReadCursor: setting?.lastReadAt || "",
      onMarkRead: roomSettings.preferenceStateFor(activeRoom).status === "ready" ? (cursor) => markChannelRead(channelId, cursor) : undefined,
      onOpenSettings: guestLocked ? undefined : () => openRoomSettings(activeRoom.id),
    };
  }

  function toggleChannelSection(sectionId: string) {
    setCollapsedChannelSections((previous) => ({
      ...previous,
      [sectionId]: !previous[sectionId],
    }));
  }

  return {
    acceptRecoveredSession, activeAppearance,
    activeChannelDisplay, activeChannelSettings,
    activeRoom, activeRoomAgentSessions, activeRoomCapabilities,
    activeRoomDisconnected, activeRoomHistory, activeRoomMembers,
    addFreshRoom, adjustSidebarWidthWithKeyboard,
    adminOpen, admittedSessionToken, agentActivityVisibility, agentCreateOpen,
    agentInviteUrl, cancelMobileShellPointer, canonicalRoom,
    changeAgentActivityVisibility, channel, channelHeaderActions,
    channelMenu, channelSearchNeedle, channelSearchQuery, channelSidebarWidth,
    closeInviteModal, closeMobileRoomInfo, closeMobileSidebar, collapsedChannelSections,
    copyAgentInviteLink, copyGuestAiPacket, copyOperatorPairingLink,
    createCompanionAiPacket,
    deviceToken, clientId, exitGuestSurface, expireGuestSession,
    generateAgentInviteLink, generateInviteLink, generateOperatorPairingLink,
    goToChannel, guestAdmissionBusy, guestAiPacketPreview, guestAiPacketStatus,
    guestExpired, guestJoinRequested, guestJoinStatus, guestJoinToken,
    guestPreflightRetryable, guestJoinRetryable,
    guestLocked, guestPanelProfile, guestRecoveryRequest, guestSession,
    handleMobileShellPointerDown, handleMobileShellPointerEnd,
    inviteCopyStatus, inviteModalAppearance,
    inviteModalRoom, invitePublicUrl, inviteRoom,
    leaveRoom, leaveRoomTarget, loadCanonicalRoomHistory, loadProviderUsage,
    lobbyPostingState, markChannelRead, markRoomRead,
    membersOpen, menuChannelDisplay, menuRoom, messageSearchChannelLabels,
    messageSearchScope, mobileRoomInfoOpen, mobileSidebarOpen,
    mobileViewportIsActive, openAgentCreate, openChannelMenu,
    openCrossChannelSearchResult, openMobileProfileFromPanel, openMobileRoomInfo, openMobileSidebar,
    openRoomMenu, openRoomSettings, operatorPairingPending, operatorPairingState,
    operatorPairingUrl, pendingGuestAvatarImage, pendingGuestDisplayName, pendingMessageSearchTarget,
    publicInviteStatus, quotaViewer,
    requestGuestJoin, retryOperatorPairing,
    roomAppearanceAssets, roomAppearances, roomDirectorySyncIssue, roomInvite,
    roomHttpAuthority, roomMenu, roomMessageSearch, roomSettings, roomSocket,
    rooms, scopedAgents, scopedMentionables, serverProductSurface,
    scopedOnlineCount, scopedViewerDisplayName, selectRoom, sendAgentConfigure,
    sendAgentControl, sendParticipantMute, setAdminOpen,
    setAgentCreateOpen, setChannelNotifications, setChannelSearchQuery,
    setGuestRecoveryRequest, setLeaveRoomTargetId, setMembersOpen,
    setMessageSearchScope, setMobileRoomInfoOpen, setMobileSidebarOpen,
    setPendingGuestAvatarImage, setPendingGuestDisplayName, setPendingMessageSearchTarget,
    setRoomMenu, setSettingsModal,
    settingsModalInitialSectionId, settingsModalRoom, shellStyle,
    showMembers,
    startInviteTunnel, startSidebarResize, startupIdentityResolved, stopInviteTunnel,
    toggleChannelSection, toggleMembers, typingIndicators, updateMemberRole,
    updateRoom, visibleChannels, visibleRoomTimelineEvents,
  };
}
export type AppController = ReturnType<typeof useAppController>;
