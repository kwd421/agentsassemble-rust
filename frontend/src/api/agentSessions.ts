import type { AgentSession } from "../types/generated/AgentSession";

export type RoomAgentSession = AgentSession;

// Agent Sessions are the room UI's only provider creation path.
export interface FrontendLiveAgentCreateRequest {
  meetingId: string;
  providerId: string;
  catalogRevision?: string;
  displayName: string;
  workspacePath: string;
  modelId?: string;
  reasoningEffort?: string;
  serviceTier?: string;
  variant?: string;
  permissionMode?: string;
  maxOutputTokens?: number;
  personaCardId?: string;
  startNow?: boolean;
}
