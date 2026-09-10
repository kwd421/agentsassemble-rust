import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import AgentProfileCard from "./AgentProfileCard";
import type { RoomAgentSession } from "../../../api";
import AgentSessionDetails, {
  type AgentSessionControlAction,
} from "../AgentSessionDetails";
import type { NativeCliProviderAvailability } from "../../../roomSocketClient";
import ProviderLogo from "../ProviderLogo";
import MemberUsage from "./MemberUsage";
import type { MemberEntry } from "./memberTypes";

export type MemberDetailModalProps = {
  entry: MemberEntry;
  onClose: () => void;
  onAgentControl?: (
    session: RoomAgentSession,
    action: AgentSessionControlAction
  ) => void | Promise<void>;
  availableProviders?: NativeCliProviderAvailability[];
  localProviderActions?: boolean;
  onAgentAvatarUpdate?: (session: RoomAgentSession, file: File, displayName: string, signal: AbortSignal) => Promise<void>;
  onAgentProfileUpdate?: (session: RoomAgentSession, settings: Record<string, string>) => void | Promise<void>;
  onAgentConfigure?: (
    session: RoomAgentSession,
    settings: Record<string, string>
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
  const visible = Boolean(entry.agent || entry.agentSession);
  useEffect(() => {
    if (!visible) return;
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, [visible]);
  if (!visible) return null;
  const DetailIcon = entry.icon;
  const usageProvider = availableProviders.find((provider) => provider.provider_kind === (entry.agentSession?.provider_kind || entry.providerKind));

  return (
    <div className="dc-modal-backdrop" role="presentation">
      <dialog ref={dialogRef} className="dc-member-detail-modal fixed inset-0 text-text-primary"
        style={{ margin: "auto" }} aria-modal="true" aria-label={entry.displayName}
        onCancel={(event) => { event.preventDefault(); onClose(); }}>
        {entry.agentSession ? <AgentProfileCard
          key={`${entry.agentSession.room_id}:${entry.agentSession.session_id}`}
          session={entry.agentSession}
          avatarImage={entry.avatarImage}
          detail={entry.fullDetail || entry.detail}
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
        </AgentProfileCard> : <>
          <header className="dc-member-detail-modal-head">
            <span className="dc-member-detail-modal-avatar" data-role={entry.role}>
              {entry.avatarImage ? <img className="dc-member-avatar-image" src={entry.avatarImage} alt="" /> :
                <ProviderLogo providerKind={entry.providerKind} size={48} fallback={<DetailIcon size={22} />} />}
            </span>
            <div className="min-w-0 flex-1">
              <h2 className="truncate preserve-words">{entry.displayName}</h2>
              <p className="truncate preserve-words">{entry.fullDetail || entry.detail}</p>
            </div>
            <button type="button" className="dc-modal-close" onClick={onClose} aria-label="멤버 정보 닫기"><X size={18} /></button>
          </header>
          <MemberUsage key={usageProvider?.id || entry.id} displayName={entry.displayName} provider={usageProvider} />
        </>}
      </dialog>
    </div>
  );
}
