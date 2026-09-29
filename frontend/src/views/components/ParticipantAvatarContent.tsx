import type { ReactNode } from "react";
import { Bot } from "lucide-react";
import ProviderLogo from "./ProviderLogo";

// Participant type is identity, independent of the editable room role.
export default function ParticipantAvatarContent({ participantType, displayName = "", avatarLabel,
  providerKind, size, fallback }: {
  participantType?: string; displayName?: string; avatarLabel?: string;
  providerKind?: string; size: number; fallback?: ReactNode;
}) {
  if (participantType === "human") {
    return <>{avatarLabel ?? Array.from(displayName.trim()).slice(0, 2).join("").toUpperCase()}</>;
  }
  return <ProviderLogo providerKind={providerKind} size={size} fallback={fallback ?? <Bot size={16} />} />;
}
