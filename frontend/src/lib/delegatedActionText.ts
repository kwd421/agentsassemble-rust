import type { LobbyEvent } from "../api";
import type { CanonicalParticipantProfile } from "./canonicalRoomProjection";

export type DelegationNameSources = {
  participantProfiles?: Record<string, CanonicalParticipantProfile>;
  channelLabels?: Record<string, string>;
};

type Delegation = NonNullable<LobbyEvent["delegation"]>;

// Korean subject/object particles from the final syllable's 받침.
function hasFinalConsonant(name: string): boolean {
  const chars = [...name.trim()];
  const code = chars[chars.length - 1]?.charCodeAt(0) ?? 0;
  if (code < 0xac00 || code > 0xd7a3) return false;
  return (code - 0xac00) % 28 !== 0;
}

export function subjectParticle(name: string): "이" | "가" {
  return hasFinalConsonant(name) ? "이" : "가";
}

export function objectParticle(name: string): "을" | "를" {
  return hasFinalConsonant(name) ? "을" : "를";
}

function participantName(profiles: Record<string, CanonicalParticipantProfile>, id: string): string {
  return profiles[id]?.displayName?.trim() || "";
}

function channelName(labels: Record<string, string>, id: string): string {
  return labels[id]?.trim() || "";
}

// A room-visible delegation line renders in Korean from the structured fields;
// the server's English content stays as the machine-readable record.
export function formatDelegatedActionText(
  event: Pick<LobbyEvent, "name" | "message" | "delegation">,
  sources: DelegationNameSources = {}
): string | null {
  const delegation: Delegation | undefined = event.delegation;
  if (!delegation) return null;
  const profiles = sources.participantProfiles || {};
  const channels = sources.channelLabels || {};
  const actor = participantName(profiles, delegation.actorParticipantId) || event.name || "에이전트";
  const owner = participantName(profiles, delegation.onBehalfOfParticipantId);
  const head = owner ? `${actor}${subjectParticle(actor)} ${owner} 대신 ` : `${actor}${subjectParticle(actor)} `;
  const channel = channelName(channels, delegation.targetId);
  const channelDisplay = channel ? `#${channel} 채널` : "채널";
  const targetParticipant = participantName(profiles, delegation.targetId);
  const agentDisplay = targetParticipant ? `${targetParticipant} 에이전트` : "에이전트";
  switch (delegation.action) {
    case "create_text_channel":
      return `${head}${channelDisplay}을 만들었어요.`;
    case "rename_text_channel":
      return `${head}${channelDisplay} 이름을 바꿨어요.`;
    case "delete_text_channel":
      return `${head}${channelDisplay}을 삭제했어요.`;
    case "set_message_pin": {
      const pinned = delegation.pinned ?? !/unpinned/i.test(event.message);
      const scope = delegation.targetChannelId && channelName(channels, delegation.targetChannelId)
        ? `#${channelName(channels, delegation.targetChannelId)} 채널의 메시지`
        : "메시지";
      return pinned ? `${head}${scope}를 고정했어요.` : `${head}${scope} 고정을 해제했어요.`;
    }
    case "set_participant_mute": {
      const target = targetParticipant || "참가자";
      // Structured mute state from the server projection, never the English
      // sentence (it contains display names). Old hosts without the flag get
      // a neutral line instead of a guessed mute/unmute.
      if (typeof delegation.muted === "boolean") {
        return delegation.muted
          ? `${head}${target}${objectParticle(target)} 음소거했어요.`
          : `${head}${target}의 음소거를 해제했어요.`;
      }
      return `${head}${target}의 음소거 상태를 바꿨어요.`;
    }
    case "start_agent":
      return `${head}${agentDisplay}를 시작했어요.`;
    case "stop_agent":
      return `${head}${agentDisplay}를 중지했어요.`;
    case "create_agent":
      return `${head}${agentDisplay}를 만들었어요.`;
    case "reconfigure_agent":
      return `${head}${agentDisplay} 설정을 바꿨어요.`;
    case "remove_agent":
      return `${head}${agentDisplay}를 삭제했어요.`;
    case "kick_participant": {
      const target = targetParticipant || "참가자";
      return `${head}${target}${objectParticle(target)} 내보냈어요.`;
    }
    case "create_human_invite":
      return `${head}초대 링크를 만들었어요.`;
    default:
      return `${head}작업을 처리했어요.`;
  }
}
