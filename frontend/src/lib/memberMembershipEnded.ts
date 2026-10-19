import { ApiError } from "./apiErrors";

export const MEMBER_MEMBERSHIP_ENDED_CODE = "member_membership_ended";

export type MembershipEndedScope = "kicked" | "exported";

export type MemberSelectError = {
  roomId: string;
  message: string;
  membershipEnded?: MembershipEndedScope;
};

export function isMembershipEndedError(error: unknown): boolean {
  return error instanceof ApiError && error.code === MEMBER_MEMBERSHIP_ENDED_CODE;
}

export function membershipEndedScopeFromEventType(type: string): MembershipEndedScope {
  return type === "participant_exported" ? "exported" : "kicked";
}

export function membershipEndedCopy(roomName: string, scope: MembershipEndedScope): { title: string; body: string } {
  const title = roomName ? `‘${roomName}’에서 내보내졌어요` : "방에서 내보내졌어요";
  if (scope === "exported") {
    return { title, body: "이 방에는 더 이상 참가할 수 없어요." };
  }
  return { title, body: "다시 참가하려면 방 관리자에게 새 초대를 받아야 해요." };
}

function terminalKey(serverId: string, meetingId: string): string {
  return `${serverId}\0${meetingId}`;
}

// Each ended room belongs to the person signed in when it ended. The current
// person is read from the shared session store, so a person switched in
// another tab never inherits this tab's ended rooms.
const terminalRooms = new Map<string, { scope: MembershipEndedScope; person: string }>();
let currentPerson: () => string = () => "";

export function bindMembershipEndedPerson(source: () => string): void {
  currentPerson = source;
}

export function markMembershipEndedTerminal(serverId: string, meetingId: string, scope: MembershipEndedScope = "kicked"): void {
  if (serverId && meetingId) terminalRooms.set(terminalKey(serverId, meetingId), { scope, person: currentPerson() });
}

export function isMembershipEndedTerminal(serverId: string, meetingId: string): boolean {
  return membershipEndedTerminalScope(serverId, meetingId) !== null;
}

export function membershipEndedTerminalScope(serverId: string, meetingId: string): MembershipEndedScope | null {
  if (!serverId || !meetingId) return null;
  const entry = terminalRooms.get(terminalKey(serverId, meetingId));
  return entry && entry.person === currentPerson() ? entry.scope : null;
}

/** The person re-joined this room (e.g. a new invite): it is no longer ended for them. */
export function clearMembershipEndedTerminal(serverId: string, meetingId: string): void {
  if (serverId && meetingId) terminalRooms.delete(terminalKey(serverId, meetingId));
}

/** Ended rooms belong to the signed-in person: a different person (or sign-out) starts clean. */
export function clearAllMembershipEndedTerminals(): void {
  terminalRooms.clear();
}

export function resetMembershipEndedTerminalForTests(): void {
  terminalRooms.clear();
}
