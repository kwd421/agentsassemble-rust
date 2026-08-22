import type { Participant } from "./types/generated/Participant";
import type { Room } from "./types/generated/Room";
import type { RoomEvent } from "./types/generated/RoomEvent";
import type { TicketResponse } from "./types/generated/TicketResponse";

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

export interface RoomAgentSession {
  room_id: string;
  session_id: string;
  participant_id: string;
  display_name: string;
  status: string;
  [key: string]: unknown;
}

export function loadHostToken(): string {
  return inMemoryHostToken;
}

export function saveHostToken(token: string): void {
  const cleanToken = token.trim();
  inMemoryHostToken = cleanToken;
}

export async function getWsTicket(auth: RoomSocketAuth): Promise<string> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  const body: Record<string, string> = {};
  if (auth.kind === "host") {
    const hostToken = loadHostToken();
    if (hostToken) headers["X-Host-Token"] = hostToken;
    body.meeting_id = auth.meetingId;
  } else {
    headers.Authorization = `Bearer ${auth.sessionToken}`;
  }
  const response = await fetch("/api/ws-ticket", {
    method: "POST",
    headers,
    body: JSON.stringify(body),
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
  const grant = await response.json() as TicketResponse;
  if (!grant.ticket) throw new Error("Ticket response did not include a ticket.");
  return grant.ticket;
}
