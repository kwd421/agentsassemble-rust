import type { AgentSession } from "../types/generated/AgentSession";

export type RoomAgentSession = AgentSession;

// Agent Sessions are the room UI's only provider creation path.
export interface FrontendLiveAgentCreateRequest {
  meetingId: string;
  sessionId?: string;
  providerId: string;
  catalogRevision?: string;
  displayName: string;
  workspacePath: string;
  providerEndpoint?: string;
  modelId?: string;
  reasoningEffort?: string;
  serviceTier?: string;
  variant?: string;
  permissionMode?: string;
  maxOutputTokens?: number;
  personaCardId?: string;
  startNow?: boolean;
}

export function agentCreationPayload(request: FrontendLiveAgentCreateRequest) {
  return {
    provider_id: request.providerId,
    catalog_revision: request.catalogRevision || "",
    display_name: request.displayName,
    workspace: request.workspacePath,
    provider_endpoint: request.providerEndpoint || "",
    model: request.modelId || "",
    reasoning_effort: request.reasoningEffort || "",
    service_tier: request.serviceTier || "",
    variant: request.variant || "",
    permission_mode: request.permissionMode || "meeting_read_only",
    max_output_tokens: request.maxOutputTokens || 0,
    persona_card_id: request.personaCardId || "",
    start: Boolean(request.startNow),
  };
}
