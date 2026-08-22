import type { Participant } from "./types/generated/Participant";
import type { AgentSession } from "./types/generated/AgentSession";
import type { Room } from "./types/generated/Room";
import type { RoomEvent } from "./types/generated/RoomEvent";
import type { TicketResponse } from "./types/generated/TicketResponse";
import {
  signHostTicketRequest,
  verifyHostChallenge,
  verifyHostTicketResponse,
  type HostChallengeGrant,
  type HostTicketGrant,
} from "./hostTicketProof";

let inMemoryHostToken = "";

export type ServerRoom = Room;
export type RoomMember = Participant;
export type { RoomEvent };

export type RoomSocketAuth =
  | { kind: "session"; sessionToken: string }
  | { kind: "host"; meetingId: string };

export interface LobbyAttachmentRef {
  id: string;
  filename: string;
  content_type: string;
  size: number;
  is_image: boolean;
  url: string;
  download_url: string;
}

export interface LobbyEvent {
  id: string;
  kind: string;
  name: string;
  message: string;
  side: string;
  created_at: string;
}

export interface LobbyPostResponse {
  event?: LobbyEvent;
  events: LobbyEvent[];
}

export interface SideChatEvent extends LobbyEvent {
  audience?: string;
  official_record?: boolean;
}

export type RoomAgentSession = AgentSession;

export function loadHostToken(): string {
  return inMemoryHostToken;
}

export function saveHostToken(token: string): void {
  const cleanToken = token.trim();
  inMemoryHostToken = cleanToken;
}

export async function getWsTicket(auth: RoomSocketAuth): Promise<TicketResponse> {
  const headers: Record<string, string> = {};
  let challenge = "";
  let hostToken = "";
  if (auth.kind === "host") {
    hostToken = loadHostToken();
    const challengeResponse = await fetch("/api/host-challenge");
    if (!challengeResponse.ok) {
      throw new Error(`${challengeResponse.status} ${challengeResponse.statusText}`);
    }
    const challengeGrant = await challengeResponse.json() as HostChallengeGrant;
    if (!(await verifyHostChallenge(hostToken, challengeGrant))) {
      throw new Error("Host challenge did not prove the server authority.");
    }
    challenge = challengeGrant.challenge;
    headers["X-Host-Challenge"] = challenge;
    headers["X-Host-Meeting"] = auth.meetingId;
    headers["X-Host-Proof"] = await signHostTicketRequest(
      hostToken,
      challenge,
      auth.meetingId
    );
  } else {
    headers.Authorization = `Bearer ${auth.sessionToken}`;
  }
  const response = await fetch("/api/ws-ticket", {
    method: "POST",
    headers,
  });
  if (!response.ok) {
    const payload = await response.json().catch(() => null) as {
      error?: { message?: string } | string;
    } | null;
    const message = typeof payload?.error === "string"
      ? payload.error
      : payload?.error?.message;
    throw new Error(message || `${response.status} ${response.statusText}`);
  }
  const grant = await response.json() as HostTicketGrant;
  if (
    auth.kind !== "host" ||
    !(await verifyHostTicketResponse(hostToken, challenge, grant))
  ) {
    throw new Error("Ticket response did not prove the host authority.");
  }
  return grant;
}
