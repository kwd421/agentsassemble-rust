import type { PointerEvent as ReactPointerEvent } from "react";
import { Bot, Code2, Crown, ShieldCheck, User } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { LiveAgent, RoomMember } from "../../../api";
import { isActivePresence, presenceStatusLabel } from "../../../lib/presenceStatus";
import type { RoleId } from "./memberTypes";

export { agentSessionResumeStatus } from "../../../lib/agentSessionStatus";

export const ROLE_OPTIONS: Array<{ id: RoleId; label: string; icon: LucideIcon }> = [
  { id: "human", label: "사람", icon: User },
  { id: "director", label: "진행", icon: Crown },
  { id: "implementer", label: "구현", icon: Code2 },
  { id: "reviewer", label: "리뷰어", icon: ShieldCheck },
  { id: "agent", label: "에이전트", icon: Bot },
];

const ROW_POINTER_MOVE_TOLERANCE = 8;

export function isPrimaryActivationPointer(event: ReactPointerEvent<HTMLElement>) {
  return event.pointerType !== "mouse" || event.button === 0;
}

export function rowTargetIsInteractive(target: EventTarget | null) {
  const element = target instanceof HTMLElement ? target : null;
  return Boolean(element?.closest("button, input, textarea, select, a, [role='dialog']"));
}

export function rowPointerMovedTooFar(start: { x: number; y: number }, event: ReactPointerEvent<HTMLElement>) {
  const movedX = Math.abs(event.clientX - start.x);
  const movedY = Math.abs(event.clientY - start.y);
  return movedX > ROW_POINTER_MOVE_TOLERANCE || movedY > ROW_POINTER_MOVE_TOLERANCE;
}

export function isActive(agent: LiveAgent) {
  return isActivePresence(agent.status);
}

export function statusDotClass(status: string) {
  if (status === "working" || status === "running") return "bg-online live-pulse";
  if (status === "online" || status === "ready") return "bg-online";
  if (status === "idle") return "bg-idle";
  if (status === "error") return "bg-danger";
  return "bg-offline";
}

export function inferAgentRole(agent: LiveAgent): RoleId {
  const text = [
    agent.binding_role_id,
    agent.display_name,
    agent.agent_id,
    agent.provider_kind,
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  if (/(director|moderator|manager|lead|owner|디렉터|총괄|책임자|팀장)/.test(text)) {
    return "director";
  }
  if (/(implement|engineer|developer|builder|coder|cursor|code|구현|개발)/.test(text)) {
    return "implementer";
  }
  if (/(review|critic|qa|xhigh|검토|리뷰)/.test(text)) {
    return "reviewer";
  }
  return "agent";
}

export function memberActive(member: RoomMember) {
  return isActivePresence(member.status);
}

export function memberRole(member: RoomMember): RoleId {
  return member.role;
}

export function memberStatusLabel(member: RoomMember) {
  return presenceStatusLabel(member.status);
}
