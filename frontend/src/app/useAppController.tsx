import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  CSSProperties,
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import {
  createCompanionRoomInvite,
  refreshProviderCatalog,
  type ChannelNotificationSetting,
  type ChannelSettings,
  type LobbyEvent,
  type RoomFriend,
  type RoomMember,
  type RoomSearchResult,
} from "../api";
import { useCanonicalRoom } from "../useCanonicalRoom";
import type {
  ChannelHeaderActions,
  ChannelSearchScope,
} from "../views/components/ChannelHeader";
import type { RoomMenuState } from "../views/components/RoomRail";
import { useRoomMessageSearch } from "../views/useRoomMessageSearch";
import { loadAgentActivityVisibility } from "../lib/agentActivityPreferences";
import { isDesktopWebview } from "../lib/desktopBridge";
import { consumeGuestRecoveryRequestFromUrl } from "../lib/guestRecovery";
import { roomAppearanceStyle } from "../lib/roomAppearance";
import {
  createStartupRoute,
  localPreviewInviteUrlForRoom,
  roomIsDisconnected,
  type RoomDockItem,
} from "../lib/roomDockModel";
import { consumeOperatorPairingTokenFromUrl } from "../lib/roomGuestSession";
import { roomPostingState } from "../lib/roomGuestPosting";
import { currentServerProductSurface } from "../lib/roomDirectoryContract";
import { remoteClientPacketPreview } from "../lib/roomInviteCopy";
import { roomRailMenuPosition } from "../lib/roomRailMenuPosition";
import type { HomeFilter } from "./friendsDirectoryTypes";
import {
  CHANNELS,
  EMPTY_ROOM,
  channelLastReadSummary,
  channelNotificationSummary,
  copyText,
  type Channel,
  type ChannelMenuState,
  type RightPanelMode,
  type RoomSettingsSectionId,
  type RoomSettingsState,
} from "./appModel";
import { mobileViewportMatches, useMobilePanels } from "./useMobilePanels";
import { useFriendsDirectory } from "./useFriendsDirectory";
import { useAgentPresentation } from "./useAgentPresentation";
import { useDismissMenus } from "./useDismissMenus";
import { useRoomAdmission } from "./useRoomAdmission";
import { useRoomChannels } from "./useRoomChannels";
import { useRoomCreation } from "./useRoomCreation";
import { useRoomDirectory } from "./useRoomDirectory";
import { useRoomInviteController } from "./useRoomInviteController";
import { useRoomMembers } from "./useRoomMembers";
import {
  useRoomSettingsController,
  type RoomPreferenceAuthority,
} from "./useRoomSettingsController";
import { useRoomSideChat } from "./useRoomSideChat";
import { useSidebarResize } from "./useSidebarResize";

export function useAppController(deviceToken: string) {
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
  // A built-in surface ("friends"/"lobby") or an opaque custom channel id.
  const [channel, setChannel] = useState<string>(() => {
    if (
      startupRoute.initialChannel === "friends" &&
      mobileViewportMatches()
    ) {
      return "lobby";
    }
    return startupRoute.initialChannel;
  });
  const [adminOpen, setAdminOpen] = useState(false);
  const [membersOpen, setMembersOpen] = useState(true);
  const [rightPanelMode, setRightPanelMode] = useState<RightPanelMode>("room-info");
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
    refreshRoomDirectory,
    verifyRoomDirectoryAuthority,
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
  const [createChannelOpen, setCreateChannelOpen] = useState(false);
  const [collapsedChannelSections, setCollapsedChannelSections] = useState<Record<string, boolean>>(
    {}
  );
  const [channelSearchQuery, setChannelSearchQuery] = useState("");
  const [rightPanelSearchQuery, setRightPanelSearchQuery] = useState("");
  const [messageSearchScope, setMessageSearchScope] = useState<ChannelSearchScope>("channel");
  const [pendingMessageSearchTarget, setPendingMessageSearchTarget] = useState<{
    channelId: string;
    eventId: string;
  } | null>(null);
  const {
    mobileSidebarOpen,
    setMobileSidebarOpen,
    mobileRoomInfoOpen,
    setMobileRoomInfoOpen,
    mobileRoomInfoInitialMode,
    setMobileRoomInfoInitialMode,
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
  } = useMobilePanels({ canOpenRoomInfo: channel !== "friends" });
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
    refreshRoomDirectory,
    verifyRoomDirectoryAuthority,
    onCreated: onRoomCreated,
  });
  const {
    payload: homeFriendsPayload,
    loading: friendsLoading,
    status: friendsStatus,
    busyId: friendsBusyId,
    homeFilter,
    friendListFilter,
    selectedFriendId: selectedHomeFriendId,
    addDraftName: friendAddDraftName,
    changeHomeFilter: changeFriendsHomeFilter,
    showDirectory: showFriendsDirectory,
    selectHomeFriend: selectFriendsHomeFriend,
    selectFriend: selectDirectoryFriend,
    openAddFriend: openFriendsAddView,
    addCandidate: addFriendsCandidate,
    addManual: addFriendsManual,
    deleteFriend: deleteDirectoryFriend,
  } = useFriendsDirectory({ enabled: startupIdentityResolved && !guestLocked });
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
  const activeSideChatMeetingId = activeOperationalMeetingId;
  const {
    error: sideChatError,
    draftsByContext: sideChatDraftsByContext,
    sideChatEvents,
    handleRealtimeEvents: handleSideChatRealtimeEvents,
    handlePostedEvents: handleSideChatPosted,
    handleRealtimeError: handleSideChatError,
    updateDraft: updateSideChatDraft,
  } = useRoomSideChat({
    meetingId: activeSideChatMeetingId,
    enabled: startupIdentityResolved && !activeRoomDisconnected,
  });
  // Rooms-as-server-objects: when a room becomes active, promote it to a
  // server-backed meeting (idempotent) so adding agents / roster / lobby always
  // have a real meeting to bind to instead of failing with "Meeting not found".
  const lobbyStreamRef = useRef<((events: LobbyEvent[]) => void) | null>(null);
  const bindLobbyStream = useCallback((receive: (events: LobbyEvent[]) => void) => {
    lobbyStreamRef.current = receive;
    return () => {
      if (lobbyStreamRef.current === receive) {
        lobbyStreamRef.current = null;
      }
    };
  }, []);
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
    onSideChat: handleSideChatRealtimeEvents,
    onError: handleSideChatError,
    onUnauthorized: admittedSessionToken ? expireGuestSession : undefined,
    onRoomDeleted: handleDeletedRoom,
  });
  const roomChannels = useRoomChannels({
    activeRoom,
    canonicalSettings: canonicalRoom.roomSettings,
    saveCanonicalSettings: canonicalRoom.sendRoomSettingsUpdate,
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
  const roomAppearances = roomSettings.appearances;
  const roomInvite = useRoomInviteController({
    deviceToken,
    guestLocked,
    sessionToken: admittedSessionToken,
  });
  const {
    modal: inviteModal,
    copyStatus: inviteCopyStatus,
    secureInviteUrl,
    agentInviteUrl,
    operatorPairingUrl,
    publicInviteStatus,
    publicUrlDraft: publicInviteUrlDraft,
    hostTokenDraft,
    friendStatuses: inviteFriendStatuses,
    remoteClientPacket: inviteRemoteClientPacket,
    invitePublicUrl,
    hostTokenRequired: inviteHostTokenRequired,
    open: openInviteModal,
    close: closeInviteModal,
    setPublicUrlDraft: setPublicInviteUrlDraft,
    setHostTokenDraft,
    configurePublicUrl: configureInvitePublicUrl,
    saveHostTokenFromDraft,
    startTunnel: startInviteTunnel,
    stopTunnel: stopInviteTunnel,
    generateSecureInvite: generateInviteLink,
    generateAgentInvite: generateAgentInviteLink,
    generateOperatorPairing: generateOperatorPairingLink,
    copyAgentInvite: copyAgentInviteLink,
    copyOperatorPairing: copyOperatorPairingLink,
    copySecureInvite: copyInviteLink,
    copyLocalPreview: copyLocalPreviewLink,
    copyRemoteClientPacket,
    inviteFriend: inviteFriendToRoom,
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
    sendParticipantKick,
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
  useEffect(() => {
    if (visibleRoomTimelineEvents.length) {
      lobbyStreamRef.current?.(visibleRoomTimelineEvents);
    }
  }, [activeRoom.meetingId, visibleRoomTimelineEvents]);
  const activeChannelSettings = roomSettings.channelSettingsFor(activeRoom);
  const activeCustomChannels = roomChannels.activeChannels;
  const activeCustomChannel = roomChannels.activeChannelFor(channel);
  const messageSearchChannelId = messageSearchScope === "all"
    ? "all"
    : channel === "lobby" || activeCustomChannel
      ? channel
      : "lobby";
  const roomMessageSearch = useRoomMessageSearch({
    roomId: activeOperationalMeetingId,
    channelId: messageSearchChannelId,
    sessionToken: admittedSessionToken,
  });
  const messageSearchChannelLabels = useMemo(
    () => Object.fromEntries([
      ["lobby", "general"],
      ...activeCustomChannels
        .filter((item) => item.type === "text")
        .map((item) => [item.id, item.name]),
    ]),
    [activeCustomChannels]
  );
  useEffect(() => {
    setMessageSearchScope("channel");
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

  function activateRightPanelMode(mode: RightPanelMode) {
    setRightPanelMode(mode);
  }

  function activateRightPanelModeFromPointer(
    mode: RightPanelMode,
    event: ReactPointerEvent<HTMLButtonElement>
  ) {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    activateRightPanelMode(mode);
  }

  function selectRoom(roomId: string) {
    setActiveRoomId(roomId);
    setAdminOpen(false);
    setChannel("lobby");
    setRightPanelMode("room-info");
    setRoomMenu(null);
    setChannelMenu(null);
    closeMobileOverlays();
  }

  function changeHomeFilter(filter: HomeFilter) {
    changeFriendsHomeFilter(filter);
  }

  function selectHomeFriend(friend: RoomFriend) {
    setChannel("friends");
    setAdminOpen(false);
    setChannelMenu(null);
    closeMobileOverlays();
    selectFriendsHomeFriend(friend);
  }

  function openAddFriendView(draftName = "") {
    setChannel("friends");
    setAdminOpen(false);
    setChannelMenu(null);
    closeMobileOverlays();
    openFriendsAddView(draftName);
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

  function handleDeletedRoom(deletedMeetingId: string) {
    const deletedRoom = rooms.find(
      (room) => room.meetingId === deletedMeetingId
    );
    if (!deletedRoom) return;
    setSettingsModal((current) =>
      current?.roomId === deletedRoom.id ? null : current
    );
    setLeaveRoomTargetId((current) =>
      current === deletedRoom.id ? "" : current
    );
    removeAcknowledgedRoom(deletedRoom.id);
    if (guestLocked) {
      exitGuestSurface();
    }
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

  async function deleteRoom(roomId: string, confirmationName: string) {
    if (roomId !== activeRoom.id || !roomSocket?.ready()) {
      throw new Error("삭제할 서버를 먼저 열고 연결이 완료될 때까지 기다려 주세요.");
    }
    await roomSocket.command("room.delete", { confirmation_name: confirmationName });
    setSettingsModal(null);
    removeAcknowledgedRoom(roomId);
  }

  function goToChannel(next: string) {
    // Guests stay out of the operator-only fixed surfaces (live/board/records/
    // friends), but custom channels are shared spaces they can enter.
    const isCustom = roomChannels.isActiveCustomChannel(next);
    const guestBlocked = guestLocked && next !== "lobby" && !isCustom;
    setChannel(guestBlocked ? "lobby" : next);
    setAdminOpen(false);
    setChannelMenu(null);
    closeMobileOverlays();
  }

  function openCrossChannelSearchResult(result: RoomSearchResult) {
    const targetChannel = result.channel_id;
    if (
      targetChannel !== "lobby"
      && !activeCustomChannels.some(
        (item) => item.id === targetChannel && item.type === "text"
      )
    ) {
      roomMessageSearch.setError("검색 결과의 채널을 더 이상 열 수 없습니다.");
      return;
    }
    setPendingMessageSearchTarget({
      channelId: targetChannel,
      eventId: result.event_id,
    });
    goToChannel(targetChannel);
  }

  async function createChannel(params: { name: string; type: "text" | "voice" }) {
    const channel = await roomChannels.create(params);
    if (channel) goToChannel(channel.id);
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
  const showMembers = !adminOpen && channel !== "friends";
  const inviteModalRoom = inviteModal ? rooms.find((room) => room.id === inviteModal.roomId) : undefined;
  const settingsModalRoom = settingsModal
    ? rooms.find((room) => room.id === settingsModal.roomId)
    : undefined;
  const leaveRoomTarget = rooms.find((room) => room.id === leaveRoomTargetId);
  const settingsModalInitialSectionId = settingsModal?.initialSectionId;
  const inviteModalAppearance = inviteModalRoom
    ? roomSettings.appearanceFor(inviteModalRoom)
    : undefined;
  const localPreviewUrl = inviteModalRoom
    ? localPreviewInviteUrlForRoom(inviteModalRoom)
    : "";
  const inviteModalMembers = inviteModalRoom
    ? roomMembers.membersFor(inviteModalRoom)
    : [];
  const activeAppearance = roomSettings.appearanceFor(activeRoom);
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
    acceptRecoveredSession, activateRightPanelMode, activateRightPanelModeFromPointer, activeAppearance,
    activeChannelDisplay, activeChannelSettings, activeCustomChannel, activeCustomChannels,
    activeRoom, activeRoomAgentSessions, activeRoomCapabilities,
    activeRoomDisconnected, activeRoomHistory, activeRoomMembers, activeSideChatMeetingId,
    addFreshRoom, addFriendsCandidate, addFriendsManual, adjustSidebarWidthWithKeyboard,
    adminOpen, admittedSessionToken, agentActivityVisibility, agentCreateOpen,
    agentInviteUrl, bindLobbyStream, cancelMobileShellPointer, canonicalRoom,
    changeAgentActivityVisibility, changeHomeFilter, channel, channelHeaderActions,
    channelMenu, channelSearchNeedle, channelSearchQuery, channelSidebarWidth,
    closeInviteModal, closeMobileRoomInfo, closeMobileSidebar, collapsedChannelSections,
    configureInvitePublicUrl, copyAgentInviteLink, copyGuestAiPacket,
    copyInviteLink, copyLocalPreviewLink, copyOperatorPairingLink, copyRemoteClientPacket,
    createChannel, createChannelOpen, createCompanionAiPacket, deleteDirectoryFriend,
    deleteRoom, deviceToken, exitGuestSurface, expireGuestSession,
    friendAddDraftName, friendListFilter, friendsBusyId, friendsLoading,
    friendsStatus, generateAgentInviteLink, generateInviteLink, generateOperatorPairingLink,
    goToChannel, guestAdmissionBusy, guestAiPacketPreview, guestAiPacketStatus,
    guestExpired, guestJoinRequested, guestJoinStatus, guestJoinToken,
    guestLocked, guestPanelProfile, guestRecoveryRequest, guestSession,
    handleMobileShellPointerDown, handleMobileShellPointerEnd, handleSideChatPosted, homeFilter,
    homeFriendsPayload, hostTokenDraft, inviteCopyStatus, inviteFriendStatuses,
    inviteFriendToRoom, inviteHostTokenRequired, inviteModalAppearance, inviteModalMembers,
    inviteModalRoom, invitePublicUrl, inviteRemoteClientPacket, inviteRoom,
    leaveRoom, leaveRoomTarget, loadCanonicalRoomHistory, loadProviderUsage,
    lobbyPostingState, localPreviewUrl, markChannelRead, markRoomRead,
    membersOpen, menuChannelDisplay, menuRoom, messageSearchChannelLabels,
    messageSearchScope, mobileRoomInfoInitialMode, mobileRoomInfoOpen, mobileSidebarOpen,
    mobileViewportIsActive, openAddFriendView, openAgentCreate, openChannelMenu,
    openCrossChannelSearchResult, openMobileProfileFromPanel, openMobileRoomInfo, openMobileSidebar,
    openRoomMenu, openRoomSettings, operatorPairingPending, operatorPairingState,
    operatorPairingUrl, pendingGuestAvatarImage, pendingGuestDisplayName, pendingMessageSearchTarget,
    publicInviteStatus, publicInviteUrlDraft, quotaViewer,
    requestGuestJoin, retryOperatorPairing, rightPanelMode,
    rightPanelSearchQuery, roomAppearances, roomDirectorySyncIssue, roomInvite,
    roomMenu, roomMessageSearch, roomSettings, roomSocket,
    rooms, saveHostTokenFromDraft, scopedAgents, scopedMentionables, serverProductSurface,
    scopedOnlineCount, scopedViewerDisplayName, secureInviteUrl, selectDirectoryFriend,
    selectHomeFriend, selectRoom, selectedHomeFriendId, sendAgentConfigure,
    sendAgentControl, sendParticipantKick, sendParticipantMute, setAdminOpen,
    setAgentCreateOpen, setChannelNotifications, setChannelSearchQuery, setCreateChannelOpen,
    setGuestRecoveryRequest, setHostTokenDraft, setLeaveRoomTargetId, setMembersOpen,
    setMessageSearchScope, setMobileRoomInfoInitialMode, setMobileRoomInfoOpen, setMobileSidebarOpen,
    setPendingGuestAvatarImage, setPendingGuestDisplayName, setPendingMessageSearchTarget, setPublicInviteUrlDraft,
    setRightPanelMode, setRightPanelSearchQuery, setRoomMenu, setSettingsModal,
    settingsModalInitialSectionId, settingsModalRoom, shellStyle, showFriendsDirectory,
    showMembers, sideChatDraftsByContext, sideChatError, sideChatEvents,
    startInviteTunnel, startSidebarResize, startupIdentityResolved, stopInviteTunnel,
    toggleChannelSection, toggleMembers, typingIndicators, updateMemberRole,
    updateRoom, updateSideChatDraft, visibleChannels, visibleRoomTimelineEvents,
  };
}
export type AppController = ReturnType<typeof useAppController>;
