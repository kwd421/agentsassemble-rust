import { Hash } from "lucide-react";
import { isCustomChannelId } from "../lib/customChannelId";
import { useRoomChannels } from "./useRoomChannels";
import { useChannelTranscript } from "./useChannelTranscript";
import { usePairedRoomLifecycle } from "./usePairedRoomLifecycle";
import { useRoomLifecycle } from "./useRoomLifecycle";
import { uploadAgentAvatar } from "../api/agentAvatar";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  CSSProperties,
  MouseEvent as ReactMouseEvent,
} from "react";
import {
  type ChannelNotificationSetting,
  type ChannelSettings,
  type RoomMember,
  type RoomAgentSession,
  type RoomSearchResult,
  type RoomEvent,
} from "../api";
import { useRoomSideChat } from "./useRoomSideChat";
import { resolveRoomHttpAuthority } from "../api/roomHttpAuthority";
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
import { roomRailMenuPosition } from "../lib/roomRailMenuPosition";
import {
  CHANNELS,
  EMPTY_ROOM,
  channelLastReadSummary,
  channelNotificationSummary,
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
    rooms, managementRooms,
    replaceRooms,
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
  const [activeRoomId, setActiveRoomId] = useState(() => startupRoute.activeRoomId);
  const [roomMenu, setRoomMenu] = useState<RoomMenuState>(null);
  const [channelMenu, setChannelMenu] = useState<ChannelMenuState>(null);
  const [settingsModal, setSettingsModal] = useState<RoomSettingsState>(null);
  const [leaveRoomTargetId, setLeaveRoomTargetId] = useState("");
  const [agentCreateOpen, setAgentCreateOpen] = useState(false);
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
    mobileViewport,
    mobileSidebarOpen,
    mobileRoomInfoOpen,
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
  const startupIdentityResolved =
    startupIdentityReady ||
    Boolean(
      startupRoute.guestInvite ||
        guestSession ||
        startupRoute.guestJoinToken ||
        operatorPairingToken ||
        guestRecoveryRequest
    );
  const hostServerProductSurface = useMemo(
    () => currentServerProductSurface(),
    [roomDirectorySyncIssue, startupIdentityResolved]
  );
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
  const roomLifecycle = useRoomLifecycle({
    enabled: !guestLocked && Boolean(serverProductSurface?.http_routes.some((route) => route.method === "POST" && route.path === "/api/rooms/lifecycle")),
    authorityReady: !roomDirectorySyncIssue,
    managementRooms, captureRoomDirectoryContinuity, validateRoomDirectoryContinuity, refreshRoomDirectory,
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
      ? ({ kind: "session" as const, sessionToken: admittedSessionToken, deviceToken })
      : undefined
    : activeRoom.meetingId
      ? ({ kind: "host" as const, meetingId: activeRoom.meetingId })
      : undefined;
  const roomSurfaceReady = Boolean(
    serverProductSurface?.websocket_streams.includes("room_events")
  );
  const sideChat = useRoomSideChat(
    startupIdentityResolved && roomSurfaceReady ? activeOperationalMeetingId : "",
    resolveRoomHttpAuthority(admittedSessionToken, !guestLocked && isDesktopWebview(), deviceToken),
  );
  const channelReceiver = useRef<((events: RoomEvent[]) => void) | null>(null);
  const canonicalRoom = useCanonicalRoom({
    roomId: startupIdentityResolved && roomSurfaceReady ? activeOperationalMeetingId : "",
    auth: roomSurfaceReady ? canonicalRoomAuth : undefined,
    streams: serverProductSurface?.websocket_streams || [],
    serverSurface: serverProductSurface,
    viewerParticipantId: guestSession?.agentId || "operator-local",
    onRoomEvents: (events) => channelReceiver.current?.(events),
    onSideChat: sideChat.receive,
    onSideChatReady: sideChat.connect,
    onSideChatClose: sideChat.disconnect,
    onUnauthorized: admittedSessionToken ? expireGuestSession : undefined,
    onRoomLifecycle: (room) => { roomLifecycle.onRoomLifecycle(); pairedRoomLifecycle.onRoomLifecycle(room); },
  });
  const roomChannels = useRoomChannels({
    activeRoom, canonicalSettings: canonicalRoom.roomSettings,
    saveCanonicalSettings: canonicalRoom.sendRoomSettingsUpdate,
  });
  const activeCustomChannel = roomChannels.activeChannels.find((item) => item.id === channel && item.type === "text") ?? null;
  const channelTranscript = useChannelTranscript({
    roomId: activeOperationalMeetingId, roomUid: canonicalRoom.room?.room_uid ?? "",
    channelId: activeCustomChannel?.id ?? "", socket: canonicalRoom.socket,
    connected: canonicalRoom.connectionState === "connected",
  });
  channelReceiver.current = channelTranscript.receive;
  useEffect(() => {
    if (isCustomChannelId(channel) && canonicalRoom.roomSettings?.roomId === activeOperationalMeetingId && !activeCustomChannel) {
      setChannel("lobby");
      setPendingMessageSearchTarget(null);
    }
  }, [channel, canonicalRoom.roomSettings, activeOperationalMeetingId, activeCustomChannel]);
  const roomMembers = useRoomMembers({
    activeRoom,
    canonicalParticipants: canonicalRoom.participants,
    enabled: startupIdentityResolved && !activeRoomDisconnected,
  });
  const roomPreferenceAuthority: RoomPreferenceAuthority = guestLocked
    ? admittedSessionToken
      ? { kind: "remote", sessionToken: admittedSessionToken, deviceToken }
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
    activeRemoteRoomId: guestLocked ? activeRoom.id : "", remoteSessionToken: admittedSessionToken, remoteDeviceToken: deviceToken,
    canonicalAppearanceFor: roomSettings.appearanceFor,
    settingsStateFor: roomSettings.settingsStateFor,
    localAuthorityCurrent: managerAuthorityCurrent,
    resolveLocalManager: resolveManagerRoomAuthority,
    bindUploadedReference: (room, slot, url) => {
      const appearance = slot === "banner" ? { bannerImage: url, bannerPreset: "custom" as const } : { iconImage: url };
      return room.roomOrigin === "remote_server"
        ? roomSettings.persist(room, { appearance })
        : roomSettings.updateAppearance(room, appearance);
    },
  });
  const roomAppearances = roomAppearanceAssets.appearances;
  const roomInvite = useRoomInviteController({
    localOperatorEligible: startupHostEnabled,
    resolveManagerRoomAuthority,
  });
  const {
    modal: inviteModal,
    copyStatus: inviteCopyStatus,
    publicInviteStatus,
    invitePublicUrl,
    open: openInviteModal,
    close: closeInviteModal,
    startTunnel: startInviteTunnel,
    stopTunnel: stopInviteTunnel,
    generateSecureInvite: generateInviteLink,
  } = roomInvite;
  const roomSocket = canonicalRoom.socket;
  const {
    activeRoomAgentSessions,
    activeRoomCapabilities,
    activeRoomHistory,
    visibleRoomTimelineEvents,
    loadCanonicalRoomHistory,
    sendAgentControl,
    sendAgentConfigure, sendAgentProfileUpdate,
    sendParticipantMute, sendParticipantRemove,
    sendParticipantRole,
    scopedAgents,
    changeAgentActivityVisibility,
    scopedMentionables,
    scopedOnlineCount,
    typingIndicators,
  } = useAgentPresentation({
    canonicalRoom,
    activeRoom,
    activeRoomMembers,
    guestSession,
    agentActivityVisibility,
    setAgentActivityVisibility,
  });
  const canManageActiveRoom = !activeRoomDisconnected && Boolean(activeRoomCapabilities["room.manage"]);
  const canControlActiveAgents = !activeRoomDisconnected && Boolean(activeRoomCapabilities["agent.control"]);
  const pairedRoomLifecycle = usePairedRoomLifecycle({
    enabled: guestLocked && canManageActiveRoom && Boolean(serverProductSurface?.http_routes.some((route) => route.method === "POST" && route.path === "/api/room-session/lifecycle")),
    session: guestSession, deviceToken, expired: guestExpired,
    room: canonicalRoom.room ? { ...canonicalRoom.room, label: canonicalRoom.roomSettings?.label ?? canonicalRoom.room.label } : null,
    refreshProjection: () => canonicalRoom.socket?.resync?.(),
  });
  useEffect(() => {
    if (!canControlActiveAgents) setAgentCreateOpen(false);
    if (guestLocked && !canManageActiveRoom) setSettingsModal(null);
  }, [canControlActiveAgents, canManageActiveRoom, guestLocked]);
  const saveAgentAvatar = useCallback(async (session: RoomAgentSession, file: File, displayName: string, signal: AbortSignal) => {
    if ((!managerAuthorityCurrent && !(guestLocked && canControlActiveAgents)) || session.room_id !== activeOperationalMeetingId || !roomSocket?.ready()) {
      throw new Error("현재 방의 에이전트 프로필 업로드 권위를 사용할 수 없습니다.");
    }
    const authority = guestLocked
      ? { kind: "remote" as const, sessionToken: admittedSessionToken, deviceToken }
      : { kind: "local" as const, manager: resolveManagerRoomAuthority(activeRoom.id) };
    const avatarUrl = await uploadAgentAvatar(file, authority, session.session_id, signal);
    signal.throwIfAborted();
    await sendAgentProfileUpdate(session, { display_name: displayName, avatar_image_url: avatarUrl });
  }, [managerAuthorityCurrent, activeOperationalMeetingId, roomSocket, resolveManagerRoomAuthority,
    activeRoom.id, sendAgentProfileUpdate, guestLocked, canControlActiveAgents, admittedSessionToken, deviceToken]);
  useDismissMenus(roomMenu, channelMenu, setRoomMenu, setChannelMenu);
  const activeChannelSettings = roomSettings.channelSettingsFor(activeRoom);
  const { roomHttpAuthority, roomMessageSearch } = useAppMessageSearch({
    roomId: activeOperationalMeetingId,
    roomUid: activeRoom.roomUid ?? "",
    selectedChannelId: channel,
    scope: messageSearchScope,
    deviceToken,
    sessionToken: admittedSessionToken,
    localAvailable: !guestLocked && isDesktopWebview(),
  });
  const visibleChannels = [...CHANNELS, ...roomChannels.activeChannels.filter((item) => item.type === "text").map((item) => ({ id: item.id, label: item.name, icon: Hash }))];
  const messageSearchChannelLabels = Object.fromEntries(visibleChannels.map((item) => [item.id, item.label]));
  useEffect(() => {
    setMessageSearchScope("all");
    setPendingMessageSearchTarget(null);
  }, [activeRoom.meetingId, activeRoom.roomUid]);
  const menuRoom = roomMenu ? rooms.find((room) => room.id === roomMenu.roomId) : undefined;
  const menuChannel = channelMenu
    ? visibleChannels.find((item) => item.id === channelMenu.channelId)
    : undefined;
  const menuChannelDisplay = menuChannel;
  const activeChannelDisplay =
    visibleChannels.find((item) => item.id === channel) ?? { id: channel, label: "채널 연결 중", icon: Hash };
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
    event.stopPropagation();
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

  const channelReadReady = Boolean(
    activeRoom.roomUid && !guestReadOnly &&
    canonicalRoom.room?.room_uid === activeRoom.roomUid &&
    canonicalRoom.connectionState === "connected" && !canonicalRoom.syncIssue &&
    canonicalRoom.roomSettings && canonicalRoom.history.initialized &&
    roomSettings.preferenceStateFor(activeRoom).status === "ready"
  );
  const roomReadReady = channelReadReady && menuRoom?.id === activeRoom.id;

  async function markRoomRead(roomId: string) {
    if (!roomReadReady || roomId !== activeRoom.id || !canonicalRoom.socket?.ready()) return;
    const cursor = `seq:${canonicalRoom.history.lastSeq}`;
    const channelIds = ["lobby", ...roomChannels.activeChannels.filter((item) => item.type === "text").map((item) => item.id)];
    try {
      await roomSettings.updateChannelSettings(activeRoom, Object.fromEntries(
        channelIds.map((channelId) => [channelId, { lastReadAt: cursor }])
      ));
      setRoomMenu((current) => current === roomMenu ? null : current);
    } catch {
      // The preference owner retains the confirmed values and exposes retry state.
    }
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
    if (!canControlActiveAgents) return;
    setAgentCreateOpen(true);
    closeMobileOverlays();
    setRoomMenu(null);
    setChannelMenu(null);
  }

  function openRoomSettings(roomId: string, initialSectionId: RoomSettingsSectionId = "settings-overview") {
    if (guestLocked && (roomId !== activeRoom.id || !canManageActiveRoom)) return;
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
    url.pathname = "/join";
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
    if (targetChannel !== "lobby" && !roomChannels.activeChannels.some((item) => item.id === targetChannel && item.type === "text")) {
      roomMessageSearch.setError("검색 결과의 채널을 더 이상 열 수 없습니다.");
      return;
    }
    setPendingMessageSearchTarget({
      channelId: targetChannel,
      eventId: result.event_id,
    });
    goToChannel(targetChannel);
  }

  const toggleMembers = useCallback(() => setMembersOpen((value) => !value), []);
  const showMembers = !adminOpen;
  const inviteModalRoom = inviteModal ? rooms.find((room) => room.id === inviteModal.roomId) : undefined;
  const settingsModalRoom = settingsModal && (!guestLocked || canManageActiveRoom)
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
    void roomSettings.updateChannelSettings(activeRoom, { [channelId]: updates }).catch(() => undefined);
  }

  function markChannelRead(channelId: string, cursor = "") {
    if (!channelReadReady || !canonicalRoom.socket?.ready()) return;
    const readCursor = cursor || `seq:${canonicalRoom.history.lastSeq}`;
    updateChannelSetting(channelId, { lastReadAt: readCursor });
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
      onMarkRead: channelReadReady ? (cursor) => markChannelRead(channelId, cursor) : undefined,
      onOpenSettings: !guestLocked || canManageActiveRoom ? () => openRoomSettings(activeRoom.id) : undefined,
    };
  }

  function toggleChannelSection(sectionId: string) {
    setCollapsedChannelSections((previous) => ({
      ...previous,
      [sectionId]: !previous[sectionId],
    }));
  }

  return {
    roomLifecycle, pairedRoomLifecycle, roomChannels, activeCustomChannel, channelTranscript,
    acceptRecoveredSession, activeAppearance,
    activeChannelDisplay, activeChannelSettings,
    canManageActiveRoom, canControlActiveAgents,
    activeRoom, activeRoomAgentSessions, activeRoomCapabilities,
    activeRoomDisconnected, activeRoomHistory, activeRoomMembers,
    addFreshRoom, adjustSidebarWidthWithKeyboard,
    adminOpen, admittedSessionToken, agentActivityVisibility, agentCreateOpen,
    cancelMobileShellPointer, canonicalRoom, sideChat,
    changeAgentActivityVisibility, channel, channelHeaderActions,
    channelMenu, channelSearchNeedle, channelSearchQuery, channelSidebarWidth,
    closeInviteModal, closeMobileRoomInfo, closeMobileSidebar, collapsedChannelSections,
    deviceToken, clientId, exitGuestSurface, expireGuestSession,
    generateInviteLink,
    goToChannel, guestAdmissionBusy,
    guestExpired, guestJoinRequested, guestJoinStatus, guestJoinToken,
    guestPreflightRetryable, guestJoinRetryable,
    guestLocked, guestPanelProfile, guestRecoveryRequest, guestSession,
    handleMobileShellPointerDown, handleMobileShellPointerEnd,
    inviteCopyStatus, inviteModalAppearance,
    inviteModalRoom, invitePublicUrl, inviteRoom,
    leaveRoom, leaveRoomTarget, loadCanonicalRoomHistory,
    lobbyPostingState, markChannelRead, channelReadReady, markRoomRead, roomReadReady,
    membersOpen, menuChannelDisplay, menuRoom, messageSearchChannelLabels,
    messageSearchScope, mobileRoomInfoOpen, mobileSidebarOpen, mobileViewport,
    openAgentCreate, openChannelMenu,
    openCrossChannelSearchResult, openMobileProfileFromPanel, openMobileRoomInfo, openMobileSidebar,
    openRoomMenu, openRoomSettings, operatorPairingPending, operatorPairingState,
    pendingGuestAvatarImage, pendingGuestDisplayName, pendingMessageSearchTarget,
    publicInviteStatus,
    requestGuestJoin, retryOperatorPairing,
    roomAppearanceAssets, roomAppearances, roomDirectorySyncIssue, roomInvite,
    roomHttpAuthority, roomMenu, roomMessageSearch, roomSettings, roomSocket,
    rooms, scopedAgents, scopedMentionables, serverProductSurface,
    saveAgentAvatar: canControlActiveAgents && (managerAuthorityCurrent || guestLocked) ? saveAgentAvatar : undefined,
    scopedOnlineCount, selectRoom, sendAgentConfigure, sendAgentProfileUpdate,
    sendAgentControl, sendParticipantMute, sendParticipantRemove, setAdminOpen,
    setAgentCreateOpen, setChannelNotifications, setChannelSearchQuery,
    setGuestRecoveryRequest, setLeaveRoomTargetId,
    setMessageSearchScope,
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
