import { createPortal } from "react-dom";

import type { AppController } from "./useAppController";
import AgentCreateModal from "../views/components/AgentCreateModal";
import GuestIdentityRecoveryPanel from "../views/components/GuestIdentityRecoveryPanel";
import GuestJoinProfilePanel from "../views/components/GuestJoinProfilePanel";
import LeaveRoomDialog from "../views/components/LeaveRoomDialog";
import RoomInviteModal from "../views/components/RoomInviteModal";
import RoomSettingsModal from "../views/components/RoomSettingsModal";

export default function AppOverlays({ controller }: { controller: AppController }) {
  const {
    acceptRecoveredSession, activeRoom, agentCreateOpen,
    canonicalRoom, closeInviteModal,
    deviceToken, clientId,
    generateInviteLink, guestAdmissionBusy, guestExpired,
    guestJoinRequested, guestJoinStatus, guestJoinToken, guestLocked,
    guestPreflightRetryable, guestJoinRetryable,
    guestRecoveryRequest, guestSession,
    inviteCopyStatus, inviteModalAppearance, inviteModalRoom, invitePublicUrl,
    inviteRoom, leaveRoom, leaveRoomTarget,
    operatorPairingPending, operatorPairingState,
    pendingGuestAvatarImage, pendingGuestDisplayName, publicInviteStatus,
    requestGuestJoin, retryOperatorPairing, roomAppearanceAssets, roomInvite,
    roomSettings, roomSocket,
    setAgentCreateOpen, setGuestRecoveryRequest,
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
            humanInvites={roomInvite.humanInvites}
            publicUrl={invitePublicUrl}
            publicAccessTransition={roomInvite.publicAccessTransition}
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
            initialSectionId={settingsModalInitialSectionId}
            appearance={roomAppearanceAssets.appearanceFor(settingsModalRoom)}
            appearanceAssetError={roomAppearanceAssets.errorFor(settingsModalRoom)}
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
            onAppearanceUpload={(file, slot) =>
              roomAppearanceAssets.upload(settingsModalRoom, file, slot)
            }
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
            onRetryAppearance={() => roomAppearanceAssets.retry(settingsModalRoom)}
          />
        )}

        <AgentCreateModal
          open={agentCreateOpen && !guestLocked}
          meetingId={activeRoom.meetingId}
          roomLabel={activeRoom.label}
          providers={canonicalRoom.availableProviders}
          catalogRevision={canonicalRoom.providerCatalog.catalog_revision}
          onClose={() => setAgentCreateOpen(false)}
          onCreate={async (request) => {
            if (!roomSocket?.ready()) {
              throw new Error("방 연결이 아직 준비되지 않았습니다");
            }
            await roomSocket.command("agent.create", {
              provider_id: request.providerId,
              catalog_revision: request.catalogRevision || "",
              display_name: request.displayName,
              workspace: request.workspacePath,
              model: request.modelId || "",
              reasoning_effort: request.reasoningEffort || "",
              service_tier: request.serviceTier || "",
              variant: request.variant || "",
              permission_mode: request.permissionMode || "meeting_read_only",
              max_output_tokens: request.maxOutputTokens || 0,
              persona_card_id: request.personaCardId || "",
              start: Boolean(request.startNow),
            });
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
