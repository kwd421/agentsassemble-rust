import { useCentralDirectory } from "./useCentralDirectory";
import { useLocalHostOwnership } from "./useLocalHostOwnership";
import { useServerOpening } from "./useServerOpening";
import { useRoomInvitationAccess } from "./useRoomInvitationAccess";
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
  applyMemberRoomLive,
  roomIsDisconnected,
  type RoomDockItem,
} from "../lib/roomDockModel";
import { useMemberRoomLive } from "./useMemberRoomLive";
import { useRailRooms } from "./useRailRooms";
import { memberRoomRemovedWhileLive } from "./useMemberHosts";
import { centralDailyQuotaNotice } from "../lib/central/centralRequestGovernor";
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
import { clearStoredCentralOwnerWorkspace } from "../lib/central/ownerWorkspace";
import { useCentralOwnerWorkspace } from "./useCentralOwnerWorkspace";
import { useRoomDirectory } from "./useRoomDirectory";
import { useRoomInviteController } from "./useRoomInviteController";
import { useRoomMembers } from "./useRoomMembers";
import {
  useRoomSettingsController,
  type RoomPreferenceAuthority,
} from "./useRoomSettingsController";
import { useSidebarResize } from "./useSidebarResize";
import { projectChannelUnread } from "../lib/channelUnread";
import { closeRemoteWorkspace, unbindRemoteSessionsForServer } from "../lib/remote/remoteWorkspace";
import { useMemberRoomAutoReselect, selectMemberRoomOnDemand } from "./useMemberRoomAutoReselect";
import { userFacingError } from "../lib/apiErrors";
import {  NO_SURFACE_STREAMS,
  applyConfirmedMembershipLost,
  canonicalRoomAuthFor,
  memberAdmissionServerFor,
  remoteRoomSessionReady,
  roomPreferenceAuthorityFor,
  serverProductSurfaceFor,
} from "./remoteRoomAuthority";

export function useAppController(deviceToken: string, clientId: string, memberReturn?: import("../lib/central/memberConnect").MemberReturn) {
  const ownerWorkspace = useCentralOwnerWorkspace(deviceToken);
  const [operatorPairingToken, setOperatorPairingToken] = useState(
    consumeOperatorPairingTokenFromUrl
  );
  const [guestRecoveryRequest, setGuestRecoveryRequest] = useState(
    consumeGuestRecoveryRequestFromUrl
  );
  const [startupRoute] = useState(() =>
    createStartupRoute({ operatorPairingPending: Boolean(operatorPairingToken), memberInviteToken: memberReturn?.record?.invite_token, memberConnect: Boolean(memberReturn?.connect || memberReturn?.record?.purpose === "connect") })
  );
  // Navigation custody only: retained admission never authorizes a server request.
  const lastOwnerRoomUid = useRef(startupRoute.guestSession?.centralOwner ? startupRoute.guestSession.roomUid || "" : "");
  const [startupIdentityReady] = useState(isDesktopWebview);
  const guestInvite = startupRoute.guestInvite;
  const guestJoinToken = startupRoute.guestJoinToken;
  // A built-in surface ("lobby") or an opaque custom channel id.
  const [channel, setChannel] = useState<string>(startupRoute.initialChannel);
  const [adminOpen, setAdminOpen] = useState(false);
  const [membersOpen, setMembersOpen] = useState(true);
  const startupHostEnabled =
    startupIdentityReady && !ownerWorkspace.connect &&
    !startupRoute.guestInvite &&
    !startupRoute.guestSession &&
    !startupRoute.guestJoinToken &&
    !startupRoute.connectorJoinUrl &&
    !operatorPairingToken &&
    !guestRecoveryRequest;
  const {
    rooms: directoryRooms, managementRooms, ownerProfileRevision, hostingRestriction,
    replaceRooms, mergeFlowRoom,
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
    remoteOwner: ownerWorkspace.remoteDirectory,
  });
  const [activeRoomId, setActiveRoomId] = useState(() => startupRoute.activeRoomId);
  // A failed member select stays visible: the rail selection still moves to
  // the clicked room and the room shows this short Korean message (never an
  // alert); the next tap retries through a fresh admission.
  const [memberSelectError, setMemberSelectError] = useState<{ roomId: string; message: string } | null>(null);
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
  const latestRooms = useRef<RoomDockItem[]>([]);
  const retainedActiveRoom = useRef<RoomDockItem | null>(null);
  const onGuestRoomJoined = useCallback((room: RoomDockItem) => {
    // A member room selected from the rail keeps its existing rail entry:
    // the anchor list carries no room_uid, so the session-derived id would
    // otherwise replace the rail id and strand the selection on a stale id.
    // A lone guest invite still replaces its placeholder.
    const existingRailId = latestRooms.current.find((existing) => existing.id === room.id ||
      (existing.roomOrigin === "remote_server" && room.roomOrigin === "remote_server" &&
        existing.serverId && existing.serverId === room.serverId && existing.meetingId === room.meetingId))?.id;
    if (ownerWorkspace.connect || existingRailId) {
      mergeFlowRoom(existingRailId && existingRailId !== room.id ? { ...room, id: existingRailId } : room);
    } else replaceRooms([room]);
    setActiveRoomId(existingRailId ?? room.id);
    setChannel("lobby");
  }, [replaceRooms, mergeFlowRoom, ownerWorkspace.connect]);
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
    guestJoinStatus, guestInviteRoomLabel,
    guestAdmissionBusy,
    guestLocked: admissionGuestLocked,
    operatorPairingPending,
    operatorPairingState,
    guestReadOnly,
    guestPanelProfile,
    setPendingGuestDisplayName,
    setPendingGuestAvatarImage,
    requestGuestJoin,
    retryOperatorPairing,
    acceptRecoveredSession, acceptOwnerSession, memberJoin, acceptMemberRoomSession,
    expireGuestSession,
    clearGuestSession,
  } = useRoomAdmission({
    memberReturn,
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
  const { directory: centralDirectory, refresh: refreshCentralDirectory } = useCentralDirectory(startupHostEnabled);
  const { localHostServerId, localOwnedByAccount } = useLocalHostOwnership(directoryRooms, centralDirectory);
  // Confirmed membership loss (kick/ban/revoke): rooms disappear from the
  // rail, their bearer clears, and a stranded selection falls back.
  const handleConfirmedMembershipLost = useCallback((serverId: string) => {
    applyConfirmedMembershipLost(serverId,
      {
        guestSession, guestExpired, latestRooms: latestRooms.current,
        activeRoomId, memberSelectError,
      },
      {
        closeHost: closeRemoteWorkspace, unbindHostSessions: unbindRemoteSessionsForServer,
        clearSession: clearGuestSession, removeRoom,
        setSelectError: setMemberSelectError, activateRoom: setActiveRoomId,
        resetChannel: () => setChannel("lobby"), closeAdmin: () => setAdminOpen(false),
        clearRetainedRoom: () => { retainedActiveRoom.current = null; },
      });
  }, [guestSession, guestExpired, clearGuestSession, removeRoom, memberSelectError, activeRoomId]);
  const { rooms: railRooms, memberHosts } = useRailRooms({
    directoryRooms,
    startupHostEnabled,
    roomDirectorySyncIssue,
    centralDirectory,
    ownerConnect: ownerWorkspace.connect,
    admittedSessionToken,
    guestSession,
    guestExpired,
    onConfirmedMembershipLost: handleConfirmedMembershipLost,
  });
  const { openRailServer, retryRoomConnection, connectingServerId, serverConnectionError } = useServerOpening({
    refreshCentralDirectory,
    refreshRoomDirectory,
    captureRoomDirectoryContinuity,
    hasMemberAnchor: memberHosts.hasAnchor,
    isMemberRefused: memberHosts.isRefused,
    retryMemberHost: memberHosts.retryHost,
  });
  if (guestSession?.centralOwner && guestSession.roomUid) lastOwnerRoomUid.current = guestSession.roomUid;
  const guestLocked = admissionGuestLocked || Boolean(ownerWorkspace.connect);
  const foundRoom = railRooms.find((room) => room.id === activeRoomId) ?? null;
  useEffect(() => {
    if (foundRoom) retainedActiveRoom.current = foundRoom;
  }, [foundRoom]);
  // The selection never silently falls back while its room is transiently
  // missing: a member re-attach briefly unlists rooms, and falling back to
  // another room then strands the click on the wrong room. The retained room
  // covers only the transient case below (an explicitly removed room always
  // resolves the selection itself); a genuinely removed member room still
  // leaves the rail once its host confirms the removal.
  const retainedRoom = !foundRoom && activeRoomId && retainedActiveRoom.current?.id === activeRoomId &&
    !memberRoomRemovedWhileLive(retainedActiveRoom.current, memberHosts.snapshots)
    ? retainedActiveRoom.current : null;
  const activeRoom = foundRoom ?? retainedRoom ?? railRooms[0] ?? EMPTY_ROOM;
  const activeRoomDisconnected = roomIsDisconnected(activeRoom);
  const activeMemberHostDetail = activeRoom.roomOrigin === "remote_server" && activeRoom.serverId
    ? memberHosts.snapshots.find((entry) => entry.serverId === activeRoom.serverId)?.detail
      // A disconnected friend room with no recorded reason still waits for
      // the daily reset while the quota block is active.
      ?? (activeRoomDisconnected && centralDirectory?.person?.person_id
        ? centralDailyQuotaNotice(centralDirectory.person.person_id) : null)
    : null;
  // A room whose select just failed opens nothing until the next tap
  // succeeds: without a session the host authority would be this computer's,
  // which must never drive another host's room.
  const activeRoomSelectError = memberSelectError && memberSelectError.roomId === activeRoom.id
    ? memberSelectError.message : null;
  // Joined member rooms stay live in the background: each covered room keeps
  // a lightweight event stream for its rail unread/mention badge. The active
  // room suspends its background stream (the canonical socket owns it).
  const memberRoomLive = useMemberRoomLive({
    snapshots: memberHosts.snapshots,
    admissionFor: memberHosts.admissionFor,
    activeServerId: activeRoom.roomOrigin === "remote_server" ? activeRoom.serverId ?? "" : "",
    activeMeetingId: activeRoom.roomOrigin === "remote_server" ? activeRoom.meetingId : "",
  });
  const rooms = useMemo(
    () => applyMemberRoomLive(railRooms, memberRoomLive.badges),
    [railRooms, memberRoomLive.badges]
  );
  latestRooms.current = rooms;
  // A member room session belongs to exactly one host room. While the local
  // host owns the surface, that session authorizes only its own room: the
  // own rooms keep host authority (and the local operator viewer) instead of
  // inheriting another host's bearer. Single-surface guest and central-owner
  // flows keep the legacy global lock untouched.
  const scopeSessionToActiveRoom = startupHostEnabled && !ownerWorkspace.connect;
  const activeRoomSession = guestSession && !guestExpired &&
    guestSession.serverSurface.server_id === activeRoom.serverId &&
    guestSession.meetingId === activeRoom.meetingId
    ? guestSession : null;
  const activeSessionToken = scopeSessionToActiveRoom
    ? (activeRoomSession ? admittedSessionToken : "")
    : admittedSessionToken;
  const activeGuestSession = scopeSessionToActiveRoom ? activeRoomSession : guestSession;
  const activeGuestLocked = scopeSessionToActiveRoom ? Boolean(activeRoomSession) : guestLocked;
  // Host auth never drives a remote host's room (see remoteRoomAuthority):
  // without its own member session the room stays disconnected.
  const activeRoomSessionReady = remoteRoomSessionReady(activeRoom, {
    guestLocked: activeGuestLocked, sessionToken: activeSessionToken,
    session: activeRoomSession, expired: guestExpired,
  });
  const activeRoomOperationallyDisconnected = activeRoomDisconnected || !activeRoomSessionReady;
  useEffect(() => {
    if (!import.meta.env.DEV) return;
    console.debug("[member-select] active room resolved", {
      activeRoomId, resolvedId: activeRoom.id, meetingId: activeRoom.meetingId,
      serverId: activeRoom.serverId ?? null,
      session: activeRoomSession
        ? { serverId: activeRoomSession.serverSurface.server_id, meetingId: activeRoomSession.meetingId }
        : null,
      authority: activeGuestLocked ? "session" : "host",
    });
  }, [activeRoomId, activeRoom.id, activeRoom.meetingId, activeRoom.serverId, activeRoomSession, activeGuestLocked]);
  const canCreateRoom = (startupHostEnabled && localOwnedByAccount) || Boolean(ownerWorkspace.connect);
  // Inviting to a joined member room goes through that host's own admission,
  // never this computer's host authority, so it stays available even when the
  // local server is not mine. Only creating new local rooms needs ownership.
  const canInviteRooms = startupHostEnabled || Boolean(ownerWorkspace.connect);
  const startupIdentityResolved =
    startupIdentityReady || Boolean(ownerWorkspace.connect) ||
    Boolean(
      startupRoute.guestInvite ||
        guestSession ||
        startupRoute.guestJoinToken ||
        startupRoute.connectorJoinUrl ||
        operatorPairingToken ||
        guestRecoveryRequest
    );
  const hostServerProductSurface = useMemo(() => {
    // The own host's surface is bound under this computer's origin. The
    // default origin follows the single live remote workspace once a member
    // host is installed, so it must not be used for the own host here: after
    // any member admission the lookup would return the member's surface.
    if (startupHostEnabled) return currentServerProductSurface(window.location.origin);
    return currentServerProductSurface();
  },
    [roomDirectorySyncIssue, startupIdentityResolved, startupHostEnabled]
  );
  const serverProductSurface = serverProductSurfaceFor(
    activeRoomSessionReady, activeRoomSession, guestSession,
    scopeSessionToActiveRoom, hostServerProductSurface);
  const onRoomCreated = useCallback(
    async (room: RoomDockItem) => {
      if (ownerWorkspace.connect && !await ownerWorkspace.enter(room, acceptOwnerSession)) return;
      setActiveRoomId(room.id);
      setAdminOpen(false);
      setChannel("lobby");
      setRoomMenu(null);
      setChannelMenu(null);
      closeMobileOverlays();
    },
    [closeMobileOverlays, ownerWorkspace.connect, ownerWorkspace.enter, acceptOwnerSession]
  );
  const { addFreshRoom } = useRoomCreation({
    guestLocked: !canCreateRoom,
    create: ownerWorkspace.create,
    captureRoomDirectoryContinuity,
    validateRoomDirectoryContinuity,
    refreshRoomDirectory,
    verifyRoomDirectoryAuthority,
    onCreated: onRoomCreated,
  });
  const ownerLifecycleTransport = useMemo(() => ownerWorkspace.connect ? {
    kind: "server_owner" as const,
    credential: { sessionToken: ownerWorkspace.connect.sessionToken, generation: ownerWorkspace.connect.generation }, deviceToken,
  } : undefined, [ownerWorkspace.connect, deviceToken]);
  const roomLifecycle = useRoomLifecycle({
    enabled: Boolean(ownerLifecycleTransport || !activeGuestLocked) && Boolean(serverProductSurface?.http_routes.some((route) => route.method === "POST" && route.path === (ownerLifecycleTransport ? "/api/room-session/lifecycle" : "/api/rooms/lifecycle"))),
    transport: ownerLifecycleTransport,
    authorityReady: !roomDirectorySyncIssue,
    managementRooms, captureRoomDirectoryContinuity, validateRoomDirectoryContinuity, refreshRoomDirectory,
  });
  const lobbyPostingState = useMemo(
    () =>
      roomPostingState({
        guestLocked: activeGuestLocked,
        guestReadOnly,
        sessionToken: activeSessionToken,
      }),
    [activeSessionToken, activeGuestLocked, guestReadOnly]
  );
  const activeOperationalMeetingId = activeRoomOperationallyDisconnected || activeRoomSelectError ? "" : activeRoom.meetingId;
  // Rooms-as-server-objects: when a room becomes active, promote it to a
  // server-backed meeting (idempotent) so adding agents / roster / lobby always
  // have a real meeting to bind to instead of failing with "Meeting not found".
  const canonicalRoomAuth = canonicalRoomAuthFor(
    activeRoom, {
      operational: activeRoomOperationallyDisconnected, selectError: Boolean(activeRoomSelectError),
      guestLocked: activeGuestLocked,
    }, activeSessionToken, deviceToken);
  const roomSurfaceReady = Boolean(
    serverProductSurface?.websocket_streams.includes("room_events")
  );
  const sideChat = useRoomSideChat(
    startupIdentityResolved && roomSurfaceReady ? activeOperationalMeetingId : "",
    resolveRoomHttpAuthority(activeSessionToken, !activeGuestLocked && activeRoomSessionReady && isDesktopWebview(), deviceToken),
  );
  const channelReceiver = useRef<((events: RoomEvent[]) => void) | null>(null);
  const canonicalRoom = useCanonicalRoom({
    roomId: startupIdentityResolved && roomSurfaceReady ? activeOperationalMeetingId : "",
    auth: roomSurfaceReady ? canonicalRoomAuth : undefined,
    streams: serverProductSurface?.websocket_streams ?? NO_SURFACE_STREAMS,
    serverSurface: serverProductSurface,
    viewerParticipantId: activeGuestSession?.agentId || "operator-local",
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
    enabled: startupIdentityResolved && !activeRoomOperationallyDisconnected,
  });
  const roomPreferenceAuthority: RoomPreferenceAuthority = roomPreferenceAuthorityFor(
    activeRoomSessionReady, activeGuestLocked, activeSessionToken, deviceToken);
  const activeRoomMembers = roomMembers.activeMembers;
  const roomSettings = useRoomSettingsController({
    activeRoom,
    preferenceAuthority: roomPreferenceAuthority,
    canonicalGlobalSettings: canonicalRoom.roomSettings,
    saveCanonicalGlobalSettings: canonicalRoom.sendRoomSettingsUpdate,
    onRoomMetadataLoaded: updateRoomByMeetingId,
    enabled: startupIdentityResolved && !activeRoomOperationallyDisconnected,
  });
  const roomAppearanceAssets = useRoomAppearanceAssets({
    rooms, activeRoomId: activeRoom.id,
    activeRemoteRoomId: activeGuestLocked ? activeRoom.id : "", remoteSessionToken: activeSessionToken, remoteDeviceToken: deviceToken,
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
  const invitationAccess = useRoomInvitationAccess(Boolean(ownerWorkspace.connect), rooms,
    guestSession, deviceToken, resolveManagerRoomAuthority);
  const roomInvite = useRoomInviteController({
    localOperatorEligible: startupHostEnabled,
    remote: invitationAccess.remote,
    resolveManagerRoomAuthority: invitationAccess.resolve,
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
    guestSession: activeGuestSession,
    agentActivityVisibility,
    setAgentActivityVisibility,
  });
  const canManageActiveRoom = !activeRoomOperationallyDisconnected && Boolean(activeRoomCapabilities["room.manage"]);
  const canControlActiveAgents = !activeRoomOperationallyDisconnected && Boolean(activeRoomCapabilities["agent.control"]);
  const canAddCompanion = !activeRoomOperationallyDisconnected && !guestExpired && Boolean(activeGuestSession) &&
    activeGuestSession?.meetingId === activeRoom.meetingId && canonicalRoom.connectionState === "connected" &&
    lobbyPostingState.canPost && Boolean(activeRoomCapabilities["message.send"]) &&
    canonicalRoom.participants.some((participant) => participant.participant_id === activeGuestSession?.agentId &&
      participant.participant_type === "human" && participant.status === "joined" && !participant.muted);
  const canAddActiveAI = canControlActiveAgents || canAddCompanion;
  const pairedRoomLifecycle = usePairedRoomLifecycle({
    enabled: !ownerLifecycleTransport && activeGuestLocked && canManageActiveRoom && Boolean(serverProductSurface?.http_routes.some((route) => route.method === "POST" && route.path === "/api/room-session/lifecycle")),
    session: activeGuestSession, deviceToken, expired: guestExpired,
    room: canonicalRoom.room ? { ...canonicalRoom.room, label: canonicalRoom.roomSettings?.label ?? canonicalRoom.room.label } : null,
    refreshProjection: () => canonicalRoom.socket?.resync?.(),
  });
  useEffect(() => {
    if (!canAddActiveAI) setAgentCreateOpen(false);
    if (activeGuestLocked && !canManageActiveRoom) setSettingsModal(null);
  }, [canAddActiveAI, canManageActiveRoom, activeGuestLocked]);
  const saveAgentAvatar = useCallback(async (session: RoomAgentSession, file: File, displayName: string, signal: AbortSignal) => {
    if ((!managerAuthorityCurrent && !(activeGuestLocked && canControlActiveAgents)) || session.room_id !== activeOperationalMeetingId || !roomSocket?.ready()) {
      throw new Error("현재 방의 에이전트 프로필 업로드 권위를 사용할 수 없습니다.");
    }
    if (!activeRoomSessionReady) {
      throw new Error("현재 방의 에이전트 프로필 업로드 권위를 사용할 수 없습니다.");
    }
    const authority = activeGuestLocked
      ? { kind: "remote" as const, sessionToken: activeSessionToken, deviceToken }
      : { kind: "local" as const, manager: resolveManagerRoomAuthority(activeRoom.id) };
    const avatarUrl = await uploadAgentAvatar(file, authority, session.session_id, signal);
    signal.throwIfAborted();
    await sendAgentProfileUpdate(session, { display_name: displayName, avatar_image_url: avatarUrl });
  }, [managerAuthorityCurrent, activeOperationalMeetingId, roomSocket, resolveManagerRoomAuthority,
    activeRoom.id, sendAgentProfileUpdate, activeGuestLocked, canControlActiveAgents, activeSessionToken, deviceToken]);
  useDismissMenus(roomMenu, channelMenu, setRoomMenu, setChannelMenu);
  const activeChannelSettings = roomSettings.channelSettingsFor(activeRoom);
  const { roomHttpAuthority, roomMessageSearch } = useAppMessageSearch({
    roomId: activeOperationalMeetingId,
    roomUid: activeRoom.roomUid ?? "",
    selectedChannelId: channel,
    scope: messageSearchScope,
    deviceToken,
    sessionToken: activeSessionToken,
    localAvailable: !activeGuestLocked && activeRoomSessionReady && isDesktopWebview(),
  });
  const visibleChannels = [...CHANNELS.map(item => item.id === "lobby" && canonicalRoom.roomSettings?.roomId === activeRoom.meetingId && canonicalRoom.roomSettings.lobbyName ? { ...item, label: canonicalRoom.roomSettings.lobbyName } : item), ...roomChannels.activeChannels.filter((item) => item.type === "text").map((item) => ({ id: item.id, label: item.name, icon: Hash }))];
  const channelUnread = useMemo(() => projectChannelUnread({
    channelIds: visibleChannels.map((item) => item.id),
    cursors: canonicalRoom.channelMessageCursors,
    eventLists: [canonicalRoom.events, channelTranscript.events],
    settings: activeChannelSettings,
  }), [visibleChannels, canonicalRoom.channelMessageCursors, canonicalRoom.events, channelTranscript.events, activeChannelSettings]);
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

  const { markAdmissionSeen } = useMemberRoomAutoReselect({
    activeRoom,
    ownerServerId: ownerWorkspace.connect?.serverId,
    hasAnchor: memberHosts.hasAnchor,
    isRefused: memberHosts.isRefused,
    snapshots: memberHosts.snapshots,
    admissionFor: memberHosts.admissionFor,
    guestSession,
    guestExpired,
    admittedSessionToken,
    liveSessionFor: memberRoomLive.liveSessionFor,
    selectRoom: memberHosts.selectRoom,
    acceptSession: acceptMemberRoomSession,
    setSelectError: setMemberSelectError,
  });

  async function selectRoom(roomId: string) {
    const room = rooms.find(item => item.id === roomId);
    // A stale rail id is still a failed click: record the short Korean
    // failure instead of abandoning the tap in silence.
    if (!room) {
      setMemberSelectError({ roomId, message: "방을 열지 못했어요. 다시 시도해 주세요." });
      return;
    }
    try {
      // An anchored member room needs its own room session before it can
      // open: select it on demand through the live host admission. Rooms
      // without an anchor keep the legacy guest/owner path untouched.
      const memberServerId = memberAdmissionServerFor(room, ownerWorkspace.connect?.serverId, memberHosts.hasAnchor);
      if (import.meta.env.DEV) console.debug("[member-select] rail select", {
        roomId, meetingId: room.meetingId, serverId: room.serverId ?? null,
        roomOrigin: room.roomOrigin ?? null, memberServerId,
      });
      if (memberServerId &&
          !(admittedSessionToken && guestSession && !guestExpired &&
            guestSession.meetingId === room.meetingId &&
            guestSession.serverSurface.server_id === room.serverId)) {
        try {
          await selectMemberRoomOnDemand({
            roomId, meetingId: room.meetingId, memberServerId,
            liveSessionFor: memberRoomLive.liveSessionFor,
            selectRoom: memberHosts.selectRoom,
            acceptSession: acceptMemberRoomSession,
            setSelectError: setMemberSelectError,
          });
        } finally {
          // The manual tap consumed this admission for this room: the
          // automatic re-select must not spend a second attempt on
          // the same host channel.
          markAdmissionSeen(memberServerId, room.meetingId);
        }
      } else if (ownerWorkspace.connect && !await ownerWorkspace.enter(room, acceptOwnerSession)) return;
    } catch (error) {
      window.alert(userFacingError(error, "방을 열지 못했어요."));
      return;
    }
    setActiveRoomId(roomId);
    setAdminOpen(false);
    setChannel("lobby");
    setRoomMenu(null);
    setChannelMenu(null);
    closeMobileOverlays();
  }

  useEffect(() => {
    if (!ownerWorkspace.connect || guestSession || !rooms.length || roomDirectorySyncIssue) return;
    const next = rooms.find(room => room.roomUid !== lastOwnerRoomUid.current);
    if (next) void selectRoom(next.id);
  }, [ownerWorkspace.connect, guestSession, rooms, roomDirectorySyncIssue]);

  // The room header anchors its menu below itself; the rail and context menus open at the pointer.
  function openRoomMenu(event: ReactMouseEvent, room: RoomDockItem, anchor?: { x: number; y: number }) {
    event.preventDefault();
    event.stopPropagation();
    if (ownerWorkspace.connect && room.id !== activeRoomId) { void selectRoom(room.id); return; }
    // A joined member room never becomes active from the rail menu alone
    // (see remoteRoomAuthority): the menu still opens for background actions.
    if (!memberAdmissionServerFor(room, ownerWorkspace.connect?.serverId, memberHosts.hasAnchor)) {
      setActiveRoomId(room.id);
    }
    setAdminOpen(false);
    const position = roomRailMenuPosition(
      anchor || { x: event.clientX, y: event.clientY },
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
  const roomReadReady = (channelReadReady && menuRoom?.id === activeRoom.id) ||
    Boolean(menuRoom && menuRoom.id !== activeRoom.id && menuRoom.roomOrigin === "remote_server" &&
      menuRoom.serverId && memberRoomLive.hasLive(menuRoom.serverId, menuRoom.meetingId));

  async function markRoomRead(roomId: string) {
    const target = rooms.find((room) => room.id === roomId);
    if (target && target.id !== activeRoom.id && target.roomOrigin === "remote_server" && target.serverId &&
        memberRoomLive.hasLive(target.serverId, target.meetingId)) {
      // A background room clears its live badge without opening it.
      memberRoomLive.markRoomRead(target.serverId, target.meetingId);
      setRoomMenu((current) => current === roomMenu ? null : current);
      return;
    }
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

  async function inviteRoom(roomId: string) {
    const room = rooms.find((candidate) => candidate.id === roomId);
    if (!room || !canInviteRooms) return;
    const ownerServerId = ownerWorkspace.connect?.serverId;
    if (ownerWorkspace.connect) {
      try {
        if (!await ownerWorkspace.enter(room, acceptOwnerSession)) return;
      } catch (error) {
        window.alert(userFacingError(error, "방 초대를 열지 못했어요."));
        return;
      }
      setActiveRoomId(roomId);
      setChannel("lobby");
      setAdminOpen(false);
      closeMobileOverlays();
    } else if (memberAdmissionServerFor(room, ownerServerId, memberHosts.hasAnchor)) {
      // A joined member room opens only through its own room session.
      await selectRoom(roomId);
    } else {
      setActiveRoomId(roomId);
      setChannel("lobby");
      setAdminOpen(false);
      closeMobileOverlays();
    }
    openInviteModal(roomId);
    setRoomMenu(null);
    setChannelMenu(null);
  }

  function openAgentCreate() {
    if (!canAddActiveAI) return;
    setAgentCreateOpen(true);
    closeMobileOverlays();
    setRoomMenu(null);
    setChannelMenu(null);
  }

  async function openRoomSettings(roomId: string, initialSectionId: RoomSettingsSectionId = "settings-overview") {
    if (activeGuestLocked && (roomId !== activeRoom.id || !canManageActiveRoom)) return;
    const room = rooms.find((candidate) => candidate.id === roomId);
    if (room && memberAdmissionServerFor(room, ownerWorkspace.connect?.serverId, memberHosts.hasAnchor)) {
      // A joined member room opens only through its own room session.
      await selectRoom(roomId);
    } else {
      setActiveRoomId(roomId);
      setAdminOpen(false);
    }
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
    clearStoredCentralOwnerWorkspace();
    clearGuestSession();
    const url = new URL(window.location.href);
    url.pathname = "/join";
    url.search = "";
    url.hash = "";
    window.location.href = url.toString();
  }

  async function leaveRoom(roomId: string) {
    const leaving = rooms.find((room) => room.id === roomId);
    if (guestLocked && guestExpired && roomId === activeRoom.id) {
      if (scopeSessionToActiveRoom && leaving?.roomOrigin === "remote_server" && leaving.serverId) {
        // A dead member session drops only its own room; the own server stays.
        memberHosts.removeRoom(leaving.serverId, leaving.meetingId);
        removeAcknowledgedRoom(roomId);
        clearGuestSession();
        return;
      }
      removeAcknowledgedRoom(roomId);
      exitGuestSurface();
      return;
    }
    if (leaving?.roomOrigin === "remote_server" && leaving.serverId &&
        (roomIsDisconnected(leaving) || roomId !== activeRoom.id || !roomSocket?.ready())) {
      // A disconnected member room cannot reach its host: drop it from the
      // rail and the anchor cache. A live host re-lists rooms it still
      // shares on the next admission.
      memberHosts.removeRoom(leaving.serverId, leaving.meetingId);
      removeAcknowledgedRoom(roomId);
      return;
    }
    if (roomId !== activeRoom.id || !roomSocket?.ready()) {
      throw new Error("나갈 서버를 먼저 열고 연결이 완료될 때까지 기다려 주세요.");
    }
    await roomSocket.command("participant.leave", {});
    if (activeRoom.roomOrigin === "remote_server" && activeRoom.serverId) {
      memberHosts.removeRoom(activeRoom.serverId, activeRoom.meetingId);
    }
    removeAcknowledgedRoom(roomId);
    if (scopeSessionToActiveRoom) {
      // Leaving one room never abandons the own server surface for another
      // host's session; drop the dead member session only when it was that room's.
      if (leaving?.roomOrigin === "remote_server" && leaving.serverId &&
          guestSession?.serverSurface.server_id === leaving.serverId &&
          guestSession?.meetingId === leaving.meetingId) {
        clearGuestSession();
      }
      return;
    }
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
      roomMessageSearch.setError("검색 결과의 채널을 더 이상 열 수 없어요.");
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
  const settingsModalRoom = settingsModal && (!activeGuestLocked || canManageActiveRoom)
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
      onOpenSettings: !activeGuestLocked || canManageActiveRoom ? () => openRoomSettings(activeRoom.id) : undefined,
    };
  }

  function toggleChannelSection(sectionId: string) {
    setCollapsedChannelSections((previous) => ({
      ...previous,
      [sectionId]: !previous[sectionId],
    }));
  }

  // A session-less remote room retries through admission, never reconnect.
  const activeRoomNeedsAdmission = activeRoom.roomOrigin === "remote_server" && !activeRoomSessionReady;
  return {
    centralDirectory, refreshCentralDirectory, openRailServer, connectingServerId, serverConnectionError, retryRoomConnection,
    roomLifecycle, pairedRoomLifecycle, roomChannels, activeCustomChannel, channelTranscript,
    acceptRecoveredSession, activeAppearance,
    activeChannelDisplay, activeChannelSettings,
    canManageActiveRoom, canControlActiveAgents, canAddCompanion, canAddActiveAI,
    activeRoom, activeRoomAgentSessions, activeRoomCapabilities,
    activeRoomDisconnected: activeRoomOperationallyDisconnected, activeMemberHostDetail, activeRoomSelectError, activeRoomNeedsAdmission, activeRoomHistory, activeRoomMembers,
    addFreshRoom, adjustSidebarWidthWithKeyboard,
    adminOpen, admittedSessionToken, agentActivityVisibility, agentCreateOpen,
    cancelMobileShellPointer, canonicalRoom, sideChat,
    changeAgentActivityVisibility, channel, channelHeaderActions,
    channelMenu, channelSearchNeedle, channelSearchQuery, channelSidebarWidth, channelUnread,
    closeInviteModal, closeMobileRoomInfo, closeMobileSidebar, collapsedChannelSections,
    deviceToken, clientId, exitGuestSurface, expireGuestSession,
    generateInviteLink,
    goToChannel, guestAdmissionBusy,
    connectorJoinUrl: startupRoute.connectorJoinUrl,
    guestExpired, guestJoinRequested, guestJoinStatus, guestInviteRoomLabel, guestJoinToken,
    guestPreflightRetryable, guestJoinRetryable,
    profileAuthorityReady: (startupHostEnabled && localOwnedByAccount) || Boolean(ownerWorkspace.connect) || Boolean(admittedSessionToken), localOwnedByAccount, localHostServerId,
    ownerProfileRevision, hostingRestriction,
    ownerProfileSession: ownerWorkspace.connect ? { sessionToken: ownerWorkspace.connect.sessionToken, generation: ownerWorkspace.connect.generation } : undefined,
    guestLocked: activeGuestLocked, canCreateRoom, canInviteRooms, guestPanelProfile, guestRecoveryRequest, guestSession,
    activeRoomSession,
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
    requestGuestJoin, retryOperatorPairing, memberJoin,
    roomAppearanceAssets, roomAppearances, roomDirectorySyncIssue, roomInvite,
    roomHttpAuthority, roomMenu, roomMessageSearch, roomSettings, roomSocket,
    rooms, scopedAgents, scopedMentionables, serverProductSurface,
    saveAgentAvatar: canControlActiveAgents && (managerAuthorityCurrent || activeGuestLocked) ? saveAgentAvatar : undefined,
    scopedOnlineCount, selectRoom, memberSelectError, sendAgentConfigure, sendAgentProfileUpdate,
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
