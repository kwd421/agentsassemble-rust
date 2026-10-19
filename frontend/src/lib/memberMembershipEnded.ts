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
  const name = roomName || "방";
  if (scope === "exported") {
    return { title: "방에서 내보내졌어요", body: `‘${name}’에 더 이상 참가할 수 없어요.` };
  }
  return { title: "방에서 내보내졌어요", body: `‘${name}’에 다시 참가하려면 방 관리자에게 새 초대를 받아야 해요.` };
}

function terminalKey(serverId: string, meetingId: string): string {
  return `${serverId}\0${meetingId}`;
}

const terminalRooms = new Map<string, MembershipEndedScope>();

export function markMembershipEndedTerminal(serverId: string, meetingId: string, scope: MembershipEndedScope = "kicked"): void {
  if (serverId && meetingId) terminalRooms.set(terminalKey(serverId, meetingId), scope);
}

export function isMembershipEndedTerminal(serverId: string, meetingId: string): boolean {
  if (!serverId || !meetingId) return false;
  return terminalRooms.has(terminalKey(serverId, meetingId));
}

export function membershipEndedTerminalScope(serverId: string, meetingId: string): MembershipEndedScope | null {
  if (!serverId || !meetingId) return null;
  return terminalRooms.get(terminalKey(serverId, meetingId)) ?? null;
}

export function resetMembershipEndedTerminalForTests(): void {
  terminalRooms.clear();
}
