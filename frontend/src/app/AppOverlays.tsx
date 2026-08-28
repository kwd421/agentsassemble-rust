import { createPortal } from "react-dom";

import type { AppController } from "./useAppController";
import AgentCreateModal from "../views/components/AgentCreateModal";
import CreateChannelModal from "../views/components/CreateChannelModal";
import GuestIdentityRecoveryPanel from "../views/components/GuestIdentityRecoveryPanel";
import GuestJoinProfilePanel from "../views/components/GuestJoinProfilePanel";
import LeaveRoomDialog from "../views/components/LeaveRoomDialog";
import RoomInviteModal from "../views/components/RoomInviteModal";
import RoomSettingsModal from "../views/components/RoomSettingsModal";

export default function AppOverlays({ controller }: { controller: AppController }) {
  const {
    acceptRecoveredSession, activeRoom, agentCreateOpen, agentInviteUrl,
    canonicalRoom, closeInviteModal, copyAgentInviteLink,
    copyInviteLink, copyOperatorPairingLink, copyRemoteClientPacket,
    createChannel, createChannelOpen, deleteRoom, deviceToken, clientId, generateAgentInviteLink,
    generateInviteLink, generateOperatorPairingLink, guestAdmissionBusy, guestExpired,
    guestJoinRequested, guestJoinStatus, guestJoinToken, guestLocked,
    guestPreflightRetryable, guestJoinRetryable,
    guestRecoveryRequest, guestSession, homeFriendsPayload,
    inviteCopyStatus, inviteFriendStatuses, inviteFriendToRoom,
    inviteModalAppearance, inviteModalMembers, inviteModalRoom, invitePublicUrl,
    inviteRemoteClientPacket, inviteRoom, leaveRoom, leaveRoomTarget,
    operatorPairingPending, operatorPairingState, operatorPairingUrl,
    pendingGuestAvatarImage, pendingGuestDisplayName, publicInviteStatus,
    requestGuestJoin, retryOperatorPairing, roomInvite,
    roomSettings, roomSocket, secureInviteUrl,
    setAgentCreateOpen, setCreateChannelOpen, setGuestRecoveryRequest,
    setLeaveRoomTargetId, setPendingGuestAvatarImage, setPendingGuestDisplayName,
    setSettingsModal, settingsModalInitialSectionId, settingsModalRoom, startInviteTunnel,
    stopInviteTunnel, updateRoom,
  } = controller;

  return createPortal(
    <div data-app-overlays style={{ position: "relative", zIndex: 220 }}>
        {leaveRoomTarget && (
          <LeaveRoomDialog
            roomLabel={leaveRoomTarget.label}
            onClose={() => setLeaveRoomTargetId("")}
            onConfirm={() => leaveRoom(leaveRoomTarget.id)}
          />
        )}

        {inviteModalRoom && (
          <RoomInviteModal
            roomLabel={inviteModalRoom.label}
            secureInviteUrl={secureInviteUrl}
            agentInviteUrl={agentInviteUrl}
            operatorPairingUrl={operatorPairingUrl}
            publicUrl={invitePublicUrl}
            publicAccessTransition={roomInvite.publicAccessTransition}
            tunnelStatus={publicInviteStatus?.tunnel}
            inviteScope={inviteModalAppearance?.inviteScope || inviteModalRoom.inviteScope || "room"}
            friends={homeFriendsPayload.friends}
            members={inviteModalMembers}
            friendStatuses={inviteFriendStatuses}
            copyStatus={inviteCopyStatus}
            remoteClientPacketPreview={inviteRemoteClientPacket.preview}
            remoteClientPacketFriendName={inviteRemoteClientPacket.friendName}
            onClose={closeInviteModal}
            onGenerateSecureInvite={(options, startTunnelIfNeeded) =>
              void generateInviteLink(
                inviteModalRoom,
                inviteModalAppearance?.inviteScope || inviteModalRoom.inviteScope || "room",
                options,
                startTunnelIfNeeded
              )
            }
            onCopy={() => void copyInviteLink()}
            onGenerateAgentInvite={(startTunnelIfNeeded) =>
              void generateAgentInviteLink(inviteModalRoom, startTunnelIfNeeded)
            }
            onCopyAgentInvite={() => void copyAgentInviteLink()}
            onGenerateOperatorPairing={() => void generateOperatorPairingLink(inviteModalRoom)}
            onCopyOperatorPairing={() => void copyOperatorPairingLink()}
            onStartTunnel={() => void startInviteTunnel()}
            onStopTunnel={() => void stopInviteTunnel()}
            onCopyRemoteClientPacket={() => void copyRemoteClientPacket()}
            onInviteFriend={(friend, startTunnelIfNeeded) =>
              void inviteFriendToRoom({
                friend,
                room: inviteModalRoom,
                appearance: inviteModalAppearance,
                startTunnelIfNeeded,
              })
            }
          />
        )}

        {settingsModalRoom && (
          <RoomSettingsModal
            room={settingsModalRoom}
            initialSectionId={settingsModalInitialSectionId}
            appearance={roomSettings.appearanceFor(settingsModalRoom)}
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
            canInvite={!guestLocked}
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
            onChannelSettingChange={(channelId, updates) =>
              roomSettings.updateChannelSetting(settingsModalRoom, channelId, updates)
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
            onDeleteRoom={(confirmationName) => deleteRoom(settingsModalRoom.id, confirmationName)}
          />
        )}

        <AgentCreateModal
          open={agentCreateOpen && !guestLocked}
          meetingId={activeRoom.meetingId}
          roomLabel={activeRoom.label}
          providers={canonicalRoom.availableProviders}
          catalogRevision={canonicalRoom.providerCatalog.catalog_revision}
          existingSessions={canonicalRoom.agentSessions}
          onClose={() => setAgentCreateOpen(false)}
          onCreate={async (request) => {
            if (!roomSocket?.ready()) {
              throw new Error("방 연결이 아직 준비되지 않았습니다");
            }
            if (request.sessionId) {
              await roomSocket.command("agent.readd", {
                agent_id: request.sessionId,
                start: Boolean(request.startNow),
              });
            } else {
              await roomSocket.command("agent.create", {
                provider_id: request.providerId,
                catalog_revision: request.catalogRevision || "",
                display_name: request.displayName,
                workspace: request.workspacePath,
                model: request.modelId || "",
                provider_endpoint: request.providerEndpoint || "",
                reasoning_effort: request.reasoningEffort || "",
                service_tier: request.serviceTier || "",
                variant: request.variant || "",
                execution_harness: request.executionHarness || "builtin",
                permission_mode: request.permissionMode || "meeting_read_only",
                max_output_tokens: request.maxOutputTokens || 0,
                persona_card_id: request.personaCardId || "",
                start: Boolean(request.startNow),
              });
            }
          }}
        />

        {guestRecoveryRequest && (
          <GuestIdentityRecoveryPanel
            deviceToken={deviceToken}
            clientId={clientId}
            request={guestRecoveryRequest}
            onRecovered={(payload) => {
              void acceptRecoveredSession(payload).then((accepted) => {
                if (accepted) setGuestRecoveryRequest(null);
              });
            }}
          />
        )}

        {createChannelOpen && !guestLocked && (
          <CreateChannelModal
            onClose={() => setCreateChannelOpen(false)}
            onCreate={createChannel}
          />
        )}

        {(guestJoinToken || operatorPairingPending) &&
          (!guestSession || guestPreflightRetryable || guestJoinRetryable) &&
          !guestExpired && (
          <GuestJoinProfilePanel
            deviceToken={deviceToken}
            inviteToken={guestJoinToken}
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
