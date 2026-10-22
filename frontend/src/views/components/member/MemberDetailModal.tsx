import AgentInstructionsSettings from "../AgentInstructionsSettings";
import { useOwnerRouteHost } from "../../../app/ownerRouteHost";
import { resolveProviderPresentation } from "../providerBranding";
import ResourceImage from "../ResourceImage";
import { useEffect, useRef } from "react";
import { X } from "../../../ui/icons";
import AgentProfileCard from "./AgentProfileCard";
import type { RoomAgentSession } from "../../../api";
import AgentSessionDetails, {
  type AgentSessionControlAction,
} from "../AgentSessionDetails";
import type { NativeCliProviderAvailability } from "../../../roomSocketClient";
import ProviderLogo from "../ProviderLogo";
import MemberUsage from "./MemberUsage";
import { externalAgentProfile, type MemberEntry, type AgentProfileTarget } from "./memberTypes";

export type MemberDetailModalProps = {
  entry: MemberEntry;
  onClose: () => void;
  onAgentControl?: (
    session: RoomAgentSession,
    action: AgentSessionControlAction
  ) => void | Promise<void>;
  availableProviders?: NativeCliProviderAvailability[];
  localProviderActions?: boolean;
  onAgentAvatarUpdate?: (session: AgentProfileTarget, file: File, displayName: string, signal: AbortSignal) => Promise<void>;
  onAgentProfileUpdate?: (session: AgentProfileTarget, settings: Record<string, string>) => void | Promise<void>;
  onAgentConfigure?: (
    session: RoomAgentSession,
    settings: Record<string, string>,
    fullAccessAcknowledged?: true
  ) => void | Promise<void>;
  activityVisible?: boolean;
  onActivityVisibilityChange?: (session: RoomAgentSession, visible: boolean) => void;
};

export default function MemberDetailModal({
  entry,
  onClose,
  onAgentControl,
  availableProviders = [],
  localProviderActions = false,
  onAgentConfigure,
  onAgentProfileUpdate,
  onAgentAvatarUpdate,
  activityVisible,
  onActivityVisibilityChange,
}: MemberDetailModalProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const external = externalAgentProfile(entry);
  const ownerHost = useOwnerRouteHost();
  const visible = Boolean(entry.agent || entry.agentSession || external);
  useEffect(() => {
    if (!visible) return;
    const dialog = dialogRef.current;
    dialog?.showModal();
    // Start on the card itself, not its first control: the photo button would
    // otherwise open in its keyboard-focus state with the edit overlay showing.
    dialog?.focus();
    return () => dialog?.close();
  }, [visible]);
  if (!visible) return null;
  const DetailIcon = entry.icon;
  const usageProvider = availableProviders.find((provider) => provider.provider_kind === (entry.agentSession?.provider_kind || entry.providerKind));

  return (
    <div className="dc-modal-backdrop" role="presentation">
      <dialog ref={dialogRef} tabIndex={-1} className="dc-member-detail-modal fixed inset-0 text-text-primary"
        style={{ margin: "auto" }} aria-modal="true" aria-label={entry.displayName}
        onCancel={(event) => {
          event.preventDefault();
          // Escape during IME composition never reaches the field's own handler, so
          // hand it the Escape first: an edit in progress is discarded, not saved on close.
          const active = document.activeElement;
          if (active instanceof HTMLInputElement && event.currentTarget.contains(active)) {
            active.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
          }
          onClose();
        }}
        onClick={(event) => {
          // A modal dialog's own backdrop reports the dialog as the click target, so the
          // pointer position decides whether the click landed outside the card.
          if (event.target !== event.currentTarget) return;
          const bounds = event.currentTarget.getBoundingClientRect();
          const outside = event.clientX < bounds.left || event.clientX > bounds.right ||
            event.clientY < bounds.top || event.clientY > bounds.bottom;
          if (outside) onClose();
        }}>
        {entry.agentSession ? <AgentProfileCard
          key={`${entry.agentSession.room_id}:${entry.agentSession.session_id}`}
          session={entry.agentSession}
          avatarImage={entry.avatarImage}
          detail={[resolveProviderPresentation({ providerKind: entry.agentSession.provider_kind }).providerName, usageProvider?.controls.find(control => control.key === "model")?.options.find(option => option.value === entry.agentSession?.model)?.label].filter(Boolean).join(" · ")}
          onClose={onClose}
          onSave={onAgentProfileUpdate}
          onAvatarUpdate={onAgentAvatarUpdate}
        >
          <AgentSessionDetails
            session={entry.agentSession}
            localProviderActions={localProviderActions}
            provider={availableProviders.find(
              (provider) => provider.provider_kind === entry.agentSession?.provider_kind
            )}
            onControl={onAgentControl}
            onConfigure={onAgentConfigure}
            activityVisible={activityVisible}
            onActivityVisibilityChange={onActivityVisibilityChange}
          />
          <MemberUsage key={usageProvider?.id || entry.id} displayName={entry.displayName} provider={usageProvider} />
        </AgentProfileCard> : external ? <AgentProfileCard
          key={`${external.room_id}:${external.participant_id}`}
          session={external}
          avatarImage={entry.avatarImage}
          detail="외부 AI"
          onClose={onClose}
          onSave={onAgentProfileUpdate}
          onAvatarUpdate={onAgentAvatarUpdate}
        >{onAgentProfileUpdate && ownerHost !== undefined ? <section className="dc-member-detail-section" aria-label={`${external.display_name} 설정`}>
          <AgentInstructionsSettings roomId={external.room_id} participantId={external.participant_id} name={external.display_name} />
        </section> : null}</AgentProfileCard> : <>
          <header className="dc-member-detail-modal-head">
            <span className="dc-member-detail-modal-avatar" data-role={entry.role}>
              {entry.avatarImage ? <ResourceImage className="dc-member-avatar-image" src={entry.avatarImage} alt="" /> :
                <ProviderLogo providerKind={entry.providerKind} size={48} fallback={<DetailIcon size={22} />} />}
            </span>
            <div className="min-w-0 flex-1">
              <h2 className="truncate preserve-words">{entry.displayName}</h2>
              <p className="truncate preserve-words">{entry.fullDetail || entry.detail}</p>
            </div>
            <button type="button" className="dc-dialog-close" onClick={onClose} aria-label="멤버 정보 닫기"><X size={24} /></button>
          </header>
          <MemberUsage key={usageProvider?.id || entry.id} displayName={entry.displayName} provider={usageProvider} />
        </>}
      </dialog>
    </div>
  );
}
