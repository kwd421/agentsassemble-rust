import { lazy, Suspense, useState } from "react";
import {
  Bell,
  CalendarDays,
  ChevronDown,
  Search,
  Settings,
  UserPlus,
  UserRound,
  X,
} from "lucide-react";
import { CHANNEL_SECTIONS, DeferredViewFallback, type ChannelConfig } from "./appModel";
import type { AppController } from "./useAppController";
import AppOverlays from "./AppOverlays";
import LobbyView from "../views/LobbyView";
import RimWorldPluginView from "../views/plugins/rimworld/RimWorldPluginView";
import { RoomSocketProvider } from "../RoomSocketContext";
import ChannelContextMenu from "../views/components/ChannelContextMenu";
import RoomConnectionPanel from "../views/components/RoomConnectionPanel";
import DisconnectedRoomView from "../views/components/DisconnectedRoomView";
import MobileRoomInfoPanel from "../views/components/MobileRoomInfoPanel";
import RoomRail from "../views/components/RoomRail";
import RoomSyncNotice from "../views/components/RoomSyncNotice";
import UserPanel from "../views/components/UserPanel";
import { SIDEBAR_WIDTH_MAX, SIDEBAR_WIDTH_MIN } from "../lib/sidebarResizeModel";
import { GUEST_SESSION_EXPIRED_MESSAGE } from "../lib/apiErrors";
import { createMessageAttachmentReadOwner } from "../lib/messageAttachmentReadScheduler";

const AdminPanel = lazy(() => import("../views/AdminPanel"));

export default function AppView({ controller }: { controller: AppController }) {
  const [messageAttachmentReadOwner] = useState(
    () => createMessageAttachmentReadOwner()
  );
  const {
    activeAppearance, activeChannelDisplay,
    activeChannelSettings,
    activeRoom, activeRoomAgentSessions, activeRoomCapabilities, activeRoomDisconnected,
    activeRoomHistory, activeRoomMembers, addFreshRoom,
    adjustSidebarWidthWithKeyboard, adminOpen,
    admittedSessionToken, agentActivityVisibility, cancelMobileShellPointer,
    canonicalRoom, changeAgentActivityVisibility, channel,
    channelHeaderActions, channelMenu, channelSearchNeedle, channelSearchQuery,
    channelSidebarWidth, closeMobileRoomInfo, closeMobileSidebar, collapsedChannelSections,
    deviceToken, exitGuestSurface,
    expireGuestSession, goToChannel,
    guestExpired, guestLocked,
    guestPanelProfile, guestSession, handleMobileShellPointerDown, handleMobileShellPointerEnd,
    inviteRoom,
    loadCanonicalRoomHistory, loadProviderUsage, lobbyPostingState, markChannelRead,
    markRoomRead, membersOpen, menuChannelDisplay, menuRoom,
    messageSearchChannelLabels, messageSearchScope, mobileRoomInfoOpen,
    mobileSidebarOpen, openAgentCreate,
    openChannelMenu, openCrossChannelSearchResult, openMobileProfileFromPanel, openMobileRoomInfo,
    openMobileSidebar, openRoomMenu, openRoomSettings, pendingMessageSearchTarget,
    quotaViewer,
    roomAppearances, roomDirectorySyncIssue, roomHttpAuthority, roomMenu, roomMessageSearch,
    roomSettings, roomSocket, rooms, scopedAgents, scopedMentionables, serverProductSurface,
    scopedOnlineCount, selectRoom, sendAgentConfigure, sendAgentControl,
    sendParticipantMute, setAdminOpen, setChannelNotifications,
    setChannelSearchQuery, setLeaveRoomTargetId,
    setMessageSearchScope,
    setPendingMessageSearchTarget, setRoomMenu,
    shellStyle, showMembers, startSidebarResize, toggleChannelSection,
    toggleMembers, typingIndicators, updateMemberRole,
    visibleChannels, visibleRoomTimelineEvents,
  } = controller;
  return (
    <RoomSocketProvider socket={roomSocket}>
    <div
      className="dc-shell flex h-screen max-h-screen overflow-hidden text-text-primary"
      style={shellStyle}
      data-banner-preset={activeAppearance.bannerPreset}
      data-mobile-sidebar-open={mobileSidebarOpen}
      data-mobile-room-info-open={mobileRoomInfoOpen}
      onPointerDown={handleMobileShellPointerDown}
      onPointerUp={handleMobileShellPointerEnd}
      onPointerCancel={cancelMobileShellPointer}
    >
      <RoomSyncNotice
        issue={canonicalRoom.syncIssue || roomDirectorySyncIssue}
      />
      <RoomRail
        rooms={rooms}
        activeRoom={activeRoom}
        roomAppearances={roomAppearances}
        guestLocked={guestLocked}
        adminOpen={adminOpen}
        menuRoom={menuRoom}
        roomMenu={roomMenu}
        onSelectRoom={selectRoom}
        onAddRoom={addFreshRoom}
        onOpenRoomMenu={openRoomMenu}
        onMarkRoomRead={markRoomRead}
        onInviteRoom={inviteRoom}
        onOpenRoomSettings={openRoomSettings}
        onLeaveRoom={(roomId) => {
          setLeaveRoomTargetId(roomId);
          setRoomMenu(null);
        }}
      />

      <AppOverlays controller={controller} />
      {/* Channel sidebar */}
      <aside className="dc-sidebar flex shrink-0 flex-col" aria-label="채널 목록">
          <header className="dc-sidebar-head shrink-0" data-tone={activeRoom.tone}>
            <button
              type="button"
              className="dc-server-header-button"
              onClick={(event) => openRoomMenu(event, activeRoom)}
              onContextMenu={(event) => openRoomMenu(event, activeRoom)}
              aria-label={`${activeRoom.label} 서버 메뉴 열기`}
            >
              <span className="truncate preserve-words">{activeRoom.label}</span>
              <ChevronDown size={16} />
            </button>
            {!guestLocked && (
              <button
                type="button"
                className="dc-mobile-room-settings"
                onClick={() => openRoomSettings(activeRoom.id)}
                aria-label="서버 설정 열기"
                title="서버 설정"
              >
                <Settings size={17} />
              </button>
            )}
            <div className="dc-sidebar-banner">
              <span
                className="dc-sidebar-server-icon"
                data-has-image={Boolean(activeAppearance.iconImage)}
              >
                {activeAppearance.iconImage ? "" : activeAppearance.iconLabel || activeRoom.shortLabel}
              </span>
              <div className="min-w-0 flex-1">
                <p className="text-[10px] font-black uppercase tracking-wide text-white/70">
                  Room
                </p>
                <p className="truncate text-[12px] font-semibold text-text-muted preserve-words">
                  {activeRoom.topic}
                </p>
              </div>
              {!guestLocked && (
                <button
                  type="button"
                  className="dc-sidebar-invite-button"
                  onClick={(event) => {
                    event.stopPropagation();
                    inviteRoom(activeRoom.id);
                  }}
                  aria-label="서버에 초대하기"
                  title="서버에 초대하기"
                >
                  <UserPlus size={20} />
                </button>
              )}
            </div>
            <div className="dc-mobile-channel-tools" aria-label="모바일 채널 도구">
              <label className="dc-mobile-channel-search">
                <span className="sr-only">채널 검색</span>
                <Search size={18} />
                <input
                  type="search"
                  value={channelSearchQuery}
                  onChange={(event) => setChannelSearchQuery(event.currentTarget.value)}
                  placeholder="검색하기"
                />
              </label>
              {!guestLocked && (
                <button
                  type="button"
                  className="dc-mobile-channel-tool"
                  onClick={() => inviteRoom(activeRoom.id)}
                  aria-label="멤버 초대하기"
                  title="멤버 초대하기"
                >
                  <UserPlus size={18} />
                </button>
              )}
              <button
                type="button"
                className="dc-mobile-channel-tool"
                onClick={() => markChannelRead(channel)}
                aria-label="현재 채널 읽음으로 표시"
                title="현재 채널 읽음으로 표시"
              >
                <CalendarDays size={18} />
              </button>
            </div>
          </header>

        <nav className="min-h-0 flex-1 overflow-y-auto px-2 py-3 chat-scroll" aria-label="채널">
          {CHANNEL_SECTIONS.map((section) => {
            const channels = section.channels
              .map((id) => visibleChannels.find((item) => item.id === id))
              .filter((item) => {
                if (!item || !channelSearchNeedle) return Boolean(item);
                const display = item;
                return display.label.toLowerCase().includes(channelSearchNeedle);
              })
              .filter(Boolean) as ChannelConfig[];
            if (!channels.length) return null;
            const sectionCollapsed = Boolean(collapsedChannelSections[section.id]);
            const activeSectionChannel = channels.find((item) => item.id === channel);
            const visibleSectionChannels =
              sectionCollapsed && activeSectionChannel
                ? [activeSectionChannel]
                : sectionCollapsed
                  ? []
                  : channels;
            return (
              <section key={section.id} className="dc-channel-section">
                <button
                  type="button"
                  className="dc-channel-category dc-channel-category-button"
                  data-collapsed={sectionCollapsed}
                  aria-expanded={!sectionCollapsed}
                  onClick={() => toggleChannelSection(section.id)}
                >
                  <ChevronDown size={12} />
                  {section.label}
                </button>
                {visibleSectionChannels.map((channelConfig) => {
                  const { id, label, icon: Icon } = channelConfig;
                  return (
                    <div key={id}>
                      <button
                        type="button"
                        data-active={!adminOpen && channel === id}
                        data-muted={activeChannelSettings[id]?.notifications === "mute"}
                        data-read-at={activeChannelSettings[id]?.lastReadAt || undefined}
                        onClick={() => goToChannel(id)}
                        onContextMenu={(event) => openChannelMenu(event, id)}
                        className="dc-channel"
                      >
                        <Icon size={18} className="shrink-0 opacity-70" />
                        <span className="truncate">{label}</span>
                      </button>
                    </div>
                  );
                })}
              </section>
            );
          })}
          {menuChannelDisplay && channelMenu && (
            <ChannelContextMenu
              channelLabel={menuChannelDisplay.label}
              settings={activeChannelSettings[channelMenu.channelId]}
              preferenceStatus={roomSettings.preferenceStateFor(activeRoom).status}
              preferenceError={roomSettings.preferenceStateFor(activeRoom).error?.message || ""}
              x={channelMenu.x}
              y={channelMenu.y}
              onMarkRead={() => markChannelRead(channelMenu.channelId)}
              onSetNotifications={(notifications) =>
                setChannelNotifications(channelMenu.channelId, notifications)
              }
              onOpenSettings={() => openRoomSettings(activeRoom.id, "settings-channels")}
            />
          )}
        </nav>

        <footer className="dc-user-area shrink-0">
          <UserPanel
            onlineCount={scopedOnlineCount}
            agentCount={scopedAgents.length || 0}
            hasBackendError={Boolean(canonicalRoom.syncIssue || roomDirectorySyncIssue)}
            guestProfile={guestPanelProfile}
            profileIdentity={{
              sessionToken: admittedSessionToken,
              deviceToken,
            }}
            onGuestExit={guestExpired ? exitGuestSurface : undefined}
          />
        </footer>
        <nav className="dc-mobile-bottom-nav" aria-label="모바일 하단 탐색">
          <button type="button" onClick={() => markChannelRead(channel)}>
            <Bell size={19} />
            <span>알림</span>
          </button>
          <button type="button" onClick={openMobileProfileFromPanel}>
            <UserRound size={19} />
            <span>나</span>
          </button>
        </nav>
      </aside>
      <button
        type="button"
        className="dc-mobile-scrim"
        aria-label="사이드패널 닫기"
        tabIndex={mobileSidebarOpen ? 0 : -1}
        onClick={closeMobileSidebar}
      />
      <div
        className="dc-sidebar-resizer"
        role="separator"
        tabIndex={0}
        aria-label="좌측 패널 너비 조절"
        aria-orientation="vertical"
        aria-valuemin={SIDEBAR_WIDTH_MIN}
        aria-valuemax={SIDEBAR_WIDTH_MAX}
        aria-valuenow={channelSidebarWidth}
        onPointerDown={startSidebarResize}
        onKeyDown={adjustSidebarWidthWithKeyboard}
      />

      {/* Central channel column */}
      <main className="dc-chat flex min-w-0 flex-1 flex-col" aria-label="채널 내용">
        <Suspense fallback={<DeferredViewFallback />}>
          {activeRoomDisconnected ? (
            <DisconnectedRoomView room={activeRoom} />
          ) : adminOpen ? (
            <AdminPanel onClose={() => setAdminOpen(false)} activeMeetingId={activeRoom.meetingId} />
          ) : channel === "lobby" ? (
            canonicalRoom.roomSettings?.activityPlugin === "rimworld" &&
            (serverProductSurface?.websocket_streams as string[] | undefined)?.includes("plugin") ? (
              <RimWorldPluginView
                roomId={activeRoom.id}
                envelopes={canonicalRoom.pluginEnvelopes}
                canManage={Boolean(canonicalRoom.capabilities["room.manage"])}
                onCommand={(command) => {
                  if (!roomSocket?.ready() || !roomSocket.plugin) return;
                  roomSocket.plugin({
                    plugin_id: command.plugin_id,
                    action: command.command,
                    args: command.args,
                    revision: command.revision,
                  });
                }}
              />
            ) : (
            <LobbyView
              activeRoom={activeRoom}
              agents={scopedAgents}
              messageAttachmentReadOwner={messageAttachmentReadOwner}
              mentionables={scopedMentionables}
              roomSessionToken={lobbyPostingState.sessionToken}
              messagePinsAuthority={roomHttpAuthority}
              viewerParticipantId={guestSession?.agentId || "operator-local"}
              canManageRoom={!guestLocked && !activeRoomDisconnected}
              canPostMessages={lobbyPostingState.canPost}
              canModifyMessages={
                !activeRoomDisconnected &&
                Boolean(canonicalRoom.capabilities["message.modify"])
              }
              postingMode={lobbyPostingState.mode}
              composerDisabledReason={
                guestExpired ? GUEST_SESSION_EXPIRED_MESSAGE : lobbyPostingState.disabledReason
              }
              membersOpen={membersOpen}
              onToggleMembers={toggleMembers}
              headerActions={channelHeaderActions("lobby")}
              onOpenMobileSidebar={openMobileSidebar}
              onOpenMobileInfo={openMobileRoomInfo}
              appearance={activeAppearance}
              onGuestSessionExpired={expireGuestSession}
              typingIndicators={typingIndicators}
              canonicalEvents={visibleRoomTimelineEvents}
              canonicalHistoryReady={activeRoomHistory.initialized}
              canonicalOldestSeq={activeRoomHistory.oldestSeq}
              canonicalHasMoreHistory={activeRoomHistory.hasMoreBefore}
              canonicalWindowRevision={activeRoomHistory.windowRevision}
              participantProfiles={canonicalRoom.participantProfiles}
              searchLabel={activeRoom.label}
              loadCanonicalHistory={loadCanonicalRoomHistory}
              sharedMessageSearch={roomMessageSearch}
              messageSearchScope={messageSearchScope}
              onMessageSearchScopeChange={setMessageSearchScope}
              messageSearchChannelLabels={messageSearchChannelLabels}
              pendingSearchTargetEventId={
                pendingMessageSearchTarget?.channelId === "lobby"
                  ? pendingMessageSearchTarget.eventId
                  : ""
              }
              onSearchTargetHandled={() => setPendingMessageSearchTarget(null)}
              onOpenCrossChannelSearchResult={openCrossChannelSearchResult}
            />
            )
          ) : (
            <DeferredViewFallback />
          )}
        </Suspense>
      </main>

      {mobileRoomInfoOpen && (
        <MobileRoomInfoPanel
          room={activeRoom}
          appearance={activeAppearance}
          channelLabel={activeChannelDisplay.label}
          agents={scopedAgents}
          members={activeRoomMembers}
          viewerParticipantId={guestSession?.agentId || "operator-local"}
          displayResourceBase={canonicalRoom.displayResourceBase}
          guestLocked={guestLocked}
          onClose={closeMobileRoomInfo}
          onInvite={guestLocked ? undefined : () => inviteRoom(activeRoom.id)}
          onOpenSettings={guestLocked ? undefined : () => openRoomSettings(activeRoom.id)}
          agentSessions={activeRoomAgentSessions}
          availableProviders={canonicalRoom.availableProviders}
          capabilities={activeRoomCapabilities}
          onAgentControl={sendAgentControl}
          onAgentConfigure={sendAgentConfigure}
          agentActivityVisibility={agentActivityVisibility}
          onAgentActivityVisibilityChange={changeAgentActivityVisibility}
        />
      )}

      {/* Right panel */}
      {showMembers && membersOpen && (
        <aside
          className="dc-members hidden shrink-0 xl:flex xl:flex-col"
          aria-label="방 연결 정보"
          data-testid="room-right-panel"
        >
          <div className="dc-right-panel-header-spacer">
            <button
              type="button"
              className="dc-compact-panel-close"
              onClick={toggleMembers}
              aria-label="멤버 목록 닫기"
            >
              <X size={18} />
            </button>
          </div>
          <div className="dc-right-panel-tabs" role="tablist" aria-label="우측 패널">
            <button
              type="button"
              role="tab"
              id="room-info-panel-tab"
              data-active="true"
              aria-selected="true"
              aria-controls="room-info-panel"
            >
              방 연결 정보
            </button>
          </div>
          <section
            id="room-info-panel"
            role="tabpanel"
            aria-labelledby="room-info-panel-tab"
            className="min-h-0 flex-1"
            data-testid="room-info-panel"
          >
            <RoomConnectionPanel
              room={activeRoom}
              agents={scopedAgents}
              members={activeRoomMembers}
              roomSessionToken={admittedSessionToken}
              viewerParticipantId={guestSession?.agentId || "operator-local"}
              displayResourceBase={canonicalRoom.displayResourceBase}
              onRoleChange={updateMemberRole}
              guestLocked={guestLocked}
              channelNotifications={activeChannelSettings}
              quotaViewer={quotaViewer}
              onAgentUsageRequest={loadProviderUsage}
              onStartAddAgent={openAgentCreate}
              agentSessions={activeRoomAgentSessions}
              capabilities={activeRoomCapabilities}
              onAgentControl={sendAgentControl}
              availableProviders={canonicalRoom.availableProviders}
              onAgentConfigure={sendAgentConfigure}
              agentActivityVisibility={agentActivityVisibility}
              onAgentActivityVisibilityChange={changeAgentActivityVisibility}
              onParticipantMute={sendParticipantMute}
            />
          </section>
        </aside>
      )}
    </div>
    </RoomSocketProvider>
  );
}
