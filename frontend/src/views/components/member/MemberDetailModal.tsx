import { X } from "lucide-react";
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
  onAgentConfigure,
  activityVisible,
  onActivityVisibilityChange,
}: MemberDetailModalProps) {
  if (!entry.agent && entry.agentSession) {
    return (
      <div className="dc-modal-backdrop" role="presentation" onClick={onClose}>
        <section
          className="dc-member-detail-modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby="member-detail-title"
          onClick={(event) => event.stopPropagation()}
        >
          <header className="dc-member-detail-modal-head">
            <span className="dc-member-detail-modal-avatar" data-role={entry.role}>
              {entry.avatarImage ? (
                <img className="dc-member-avatar-image" src={entry.avatarImage} alt="" />
              ) : (
                <ProviderLogo providerKind={entry.providerKind} size={48} />
              )}
            </span>
            <div className="min-w-0 flex-1">
              <h2 id="member-detail-title" className="truncate preserve-words">
                {entry.displayName}
              </h2>
              <p className="truncate preserve-words">{entry.fullDetail || entry.detail}</p>
            </div>
            <button type="button" className="dc-modal-close" onClick={onClose} aria-label="멤버 정보 닫기">
              <X size={18} />
            </button>
          </header>
          <AgentSessionDetails
            session={entry.agentSession}
            provider={availableProviders.find(
              (provider) => provider.provider_kind === entry.agentSession?.provider_kind
            )}
            onControl={onAgentControl}
            onConfigure={onAgentConfigure}
            activityVisible={activityVisible}
            onActivityVisibilityChange={onActivityVisibilityChange}
          />
        </section>
      </div>
    );
  }

  if (!entry.agent) return null;
  const DetailIcon = entry.icon;

  return (
    <div className="dc-modal-backdrop" role="presentation" onClick={onClose}>
      <section
        className="dc-member-detail-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="member-detail-title"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="dc-member-detail-modal-head">
          <span className="dc-member-detail-modal-avatar" data-role={entry.role}>
            {entry.avatarImage ? (
              <img className="dc-member-avatar-image" src={entry.avatarImage} alt="" />
            ) : (
              <ProviderLogo
                providerKind={entry.providerKind}
                size={48}
                fallback={<DetailIcon size={22} />}
              />
            )}
          </span>
          <div className="min-w-0 flex-1">
            <h2 id="member-detail-title" className="truncate preserve-words">
              {entry.displayName}
            </h2>
            <p className="truncate preserve-words">{entry.fullDetail || entry.detail}</p>
          </div>
          <button type="button" className="dc-modal-close" onClick={onClose} aria-label="멤버 정보 닫기">
            <X size={18} />
          </button>
        </header>
        {entry.agentSession && (
          <AgentSessionDetails
            session={entry.agentSession}
            provider={availableProviders.find(
              (provider) => provider.provider_kind === entry.agentSession?.provider_kind
            )}
            onControl={onAgentControl}
            onConfigure={onAgentConfigure}
            activityVisible={activityVisible}
            onActivityVisibilityChange={onActivityVisibilityChange}
          />
        )}
        <MemberUsage displayName={entry.displayName} />
      </section>
    </div>
  );
}
