import ConnectionBanner from "../views/components/ConnectionBanner";
import { useState } from "react";
import { isDesktopWebview } from "../lib/desktopBridge";
import type { CompanionInviteControls } from "./useCompanionInvites";
import OwnComputerCreateModal from "../views/components/OwnComputerCreateModal";
import { createPortal } from "react-dom";
import { agentCreationPayload } from "../api/agentSessions";

import type { AppController } from "./useAppController";
import AgentCreateModal from "../views/components/AgentCreateModal";
import ConnectorJoinNotice from "../views/components/ConnectorJoinNotice";
import GuestJoinProfilePanel from "../views/components/GuestJoinProfilePanel";
import MemberJoinPanel from "../views/components/MemberJoinPanel";
import LeaveRoomDialog from "../views/components/LeaveRoomDialog";
import RoomInviteModal from "../views/components/RoomInviteModal";
import RoomSettingsModal from "../views/components/RoomSettingsModal";

export default function AppOverlays({ controller, companionInvites }: { controller: AppController; companionInvites: CompanionInviteControls }) {
  const [hostCreation, setHostCreation] = useState(false);
  const [memberSelected, setMemberSelected] = useState(false);
  const ownComputer = companionInvites.available && (!hostCreation || !controller.canControlActiveAgents || !(controller.guestSession?.operator || controller.guestSession?.centralOwner));
  const closeCreation = () => { setHostCreation(false); controller.setAgentCreateOpen(false); };
  const {
    activeRoom, agentCreateOpen,
    canonicalRoom, canControlActiveAgents, closeInviteModal, connectorJoinUrl,
    generateInviteLink, guestAdmissionBusy, guestExpired,
    guestJoinRequested, guestJoinStatus, guestJoinToken, guestLocked,
    guestPreflightRetryable, guestJoinRetryable,
    guestSession,
    inviteCopyStatus, inviteModalAppearance, inviteModalRoom, invitePublicUrl,
    inviteRoom, leaveRoom, leaveRoomTarget, mobileViewport,
    operatorPairingPending, operatorPairingState,
    pendingGuestAvatarImage, pendingGuestDisplayName, publicInviteStatus,
    requestGuestJoin, retryOperatorPairing, memberJoin, roomAppearanceAssets, roomInvite,
    roomLifecycle, pairedRoomLifecycle, roomSettings, roomSocket,
    setLeaveRoomTargetId, setPendingGuestAvatarImage, setPendingGuestDisplayName,
    setSettingsModal, settingsModalInitialSectionId, settingsModalRoom, startInviteTunnel,
    stopInviteTunnel, updateRoom,
  } = controller;
  const serverName = controller.centralDirectory?.servers.find((server) =>
    server.server_id === guestSession?.serverSurface.server_id)?.alias || "방을 연 컴퓨터";
  const locationChoice = companionInvites.available && canControlActiveAgents && (guestSession?.operator || guestSession?.centralOwner)
    ? <div className="dc-agent-location-choice" role="radiogroup" aria-label="어디서 실행할까요?">
        <label data-active={!ownComputer}>
          <input type="radio" name="ai-location" aria-label="서버 컴퓨터" checked={!ownComputer} onChange={() => setHostCreation(true)} />
          <strong>서버 컴퓨터</strong><span>{serverName}</span>
        </label>
        <label data-active={ownComputer}>
          <input type="radio" name="ai-location" aria-label="이 컴퓨터" checked={ownComputer} onChange={() => setHostCreation(false)} />
          <strong>이 컴퓨터</strong><span>이 앱이 실행 중인 컴퓨터</span>
        </label>
      </div> : undefined;
  // Archived and closed rooms never reach the rail, so room settings carries the list.
  const lifecycleController = roomLifecycle.enabled ? roomLifecycle : pairedRoomLifecycle.enabled ? pairedRoomLifecycle : null;

  return createPortal(
    <div data-app-overlays style={{ position: "relative", zIndex: 220 }}>
        {!agentCreateOpen && isDesktopWebview() && companionInvites.status && <div className="fixed inset-x-6 top-6"><ConnectionBanner message={companionInvites.status} /></div>}
        {leaveRoomTarget && (
          <LeaveRoomDialog
            roomLabel={leaveRoomTarget.label}
            pairedDevice={guestSession?.operator === true}
            onClose={() => setLeaveRoomTargetId("")}
            onConfirm={() => leaveRoom(leaveRoomTarget.id)}
          />
        )}

        {inviteModalRoom && (
          <RoomInviteModal
            roomLabel={inviteModalRoom.label}
            friendAuthority={controller.roomHttpAuthority}
            canControlIngress={roomInvite.canControlIngress}
            humanInvites={roomInvite.humanInvites}
            connectorInvites={roomInvite.connectorInvites}
            attendeeInvites={roomInvite.attendeeInvites}
            operatorPairings={roomInvite.pairings}
            pairingCreating={roomInvite.pairingCreating}
            onCreatePairing={() => void roomInvite.generatePairing(inviteModalRoom)}
            onCopyPairing={(key) => void roomInvite.copyPairing(key)}
            onRevokePairing={(key) => void roomInvite.revokePairing(key)}
            publicUrl={invitePublicUrl}
            publicAccessTransition={roomInvite.publicAccessTransition}
            publicAccessQuery={roomInvite.publicAccessQuery}
            onRetryPublicAccess={roomInvite.retryPublicInviteState}
            tunnelStatus={publicInviteStatus?.tunnel}
            inviteScope={inviteModalAppearance?.inviteScope || inviteModalRoom.inviteScope || "room"}
            copyStatus={inviteCopyStatus}
            onClose={closeInviteModal}
            onGenerateSecureInvite={(options, startTunnelIfNeeded) =>
              void generateInviteLink(
                inviteModalRoom,
                inviteModalAppearance?.inviteScope || inviteModalRoom.inviteScope || "room",
                options,
                startTunnelIfNeeded
              )
            }
            onCopyHumanInvite={(key) => void roomInvite.copyHumanInvite(key)}
            onRevokeHumanInvite={(key) => void roomInvite.revokeHumanInvite(key)}
            onStartTunnel={() => void startInviteTunnel()}
            onStopTunnel={() => void stopInviteTunnel()}
          />
        )}

        {settingsModalRoom && (
          <RoomSettingsModal
            room={settingsModalRoom}
            mobileViewport={mobileViewport}
            initialSectionId={settingsModalInitialSectionId}
            appearance={roomAppearanceAssets.appearanceFor(settingsModalRoom)}
            appearanceAssetError={roomAppearanceAssets.errorFor(settingsModalRoom)}
            channelOptions={controller.visibleChannels}
            channelSettings={roomSettings.channelSettingsFor(settingsModalRoom)}
            settingsStatus={roomSettings.settingsStateFor(settingsModalRoom).status}
            settingsError={roomSettings.settingsStateFor(settingsModalRoom).error?.message || ""}
            preferenceStatus={roomSettings.preferenceStateFor(settingsModalRoom).status}
            preferenceError={roomSettings.preferenceStateFor(settingsModalRoom).error?.message || ""}
            conversationMode={roomSettings.conversationModeFor(settingsModalRoom)}
            toolMode={roomSettings.toolModeFor(settingsModalRoom)}
            orderedExcludePreviousSpeaker={
              roomSettings.orderedExcludePreviousSpeakerFor(settingsModalRoom)
            }
            canInvite={controller.canInviteRooms}
            lifecycleController={lifecycleController}
            onClose={() => setSettingsModal(null)}
            onInvite={() => {
              setSettingsModal(null);
              inviteRoom(settingsModalRoom.id);
            }}
            onRoomChange={(updates) => {
              const nextRoom = { ...settingsModalRoom, ...updates };
              updateRoom(settingsModalRoom.id, updates);
              void roomSettings
                .persist(nextRoom, {
                  ...(updates.label !== undefined ? { label: updates.label } : {}),
                  ...(updates.topic !== undefined ? { topic: updates.topic } : {}),
                  ...(updates.shortLabel !== undefined
                    ? { shortLabel: updates.shortLabel }
                    : {}),
                })
                .catch(() => undefined);
            }}
            onAppearanceChange={(updates) => roomSettings.updateAppearance(settingsModalRoom, updates)}
            onAppearanceUpload={(file, slot) =>
              roomAppearanceAssets.upload(settingsModalRoom, file, slot)
            }
            onChannelSettingChange={(channelId, updates) =>
              roomSettings.updateChannelSettings(settingsModalRoom, { [channelId]: updates })
            }
            onConversationModeChange={(mode) =>
              roomSettings.updateConversationMode(settingsModalRoom, mode)
            }
            onToolModeChange={(mode) =>
              roomSettings.updateToolMode(settingsModalRoom, mode)
            }
            onOrderedExcludePreviousSpeakerChange={(exclude) =>
              roomSettings.updateOrderedExcludePreviousSpeaker(
                settingsModalRoom,
                exclude
              )
            }
            onRetrySettings={() => roomSettings.refresh(settingsModalRoom)}
            onRetryAppearance={() => roomAppearanceAssets.retry(settingsModalRoom)}
          />
        )}

        {agentCreateOpen && ownComputer && <OwnComputerCreateModal
          roomLabel={activeRoom.label} providers={canonicalRoom.availableProviders} controls={companionInvites}
          onClose={closeCreation} locationChoice={locationChoice} />}
        <AgentCreateModal
          open={agentCreateOpen && canControlActiveAgents && !ownComputer}
          locationChoice={locationChoice}
          meetingId={activeRoom.meetingId}
          roomLabel={activeRoom.label}
          providers={canonicalRoom.availableProviders}
          catalogRevision={canonicalRoom.providerCatalog.catalog_revision}
          localProviderActions={isDesktopWebview() && !guestLocked}
          existingSessions={canonicalRoom.agentSessions}
          participants={canonicalRoom.participantRecords}
          onClose={closeCreation}
          onCreate={async (request) => {
            if (!roomSocket?.ready()) {
              throw new Error("방 연결이 아직 준비되지 않았어요");
            }
            if (request.sessionId) {
              await roomSocket.command("agent.readd", {
                agent_id: request.sessionId,
                start: Boolean(request.startNow),
              });
              return;
            }
            await roomSocket.command("agent.create", agentCreationPayload(request));
          }}
        />

        {connectorJoinUrl ? <ConnectorJoinNotice joinUrl={connectorJoinUrl} /> : null}

        {(guestJoinToken || operatorPairingPending || memberJoin?.purpose === "connect") &&
          (!guestSession || guestPreflightRetryable || guestJoinRetryable) &&
          !guestExpired && (
          memberJoin && (memberSelected || memberJoin.callback) ? <MemberJoinPanel
            host={memberJoin} onCancel={memberJoin.callback ? undefined : () => setMemberSelected(false)} /> : <GuestJoinProfilePanel
            roomLabel={controller.guestInviteRoomLabel}
            onMemberJoin={memberJoin ? () => setMemberSelected(true) : undefined}
            pairing={operatorPairingPending}
            pairingState={operatorPairingState}
            retryMode={
              guestPreflightRetryable ? "preflight" : guestJoinRetryable ? "join" : undefined
            }
            displayName={pendingGuestDisplayName}
            avatarImage={pendingGuestAvatarImage || undefined}
            status={guestJoinStatus}
            busy={guestAdmissionBusy || guestJoinRequested}
            onDisplayNameChange={setPendingGuestDisplayName}
            onAvatarImageChange={setPendingGuestAvatarImage}
            onJoin={requestGuestJoin}
            onPairingRetry={retryOperatorPairing}
          />
        )}

    </div>,
    document.body
  );
}
