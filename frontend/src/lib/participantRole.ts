import type { ParticipantRole } from "../types/generated/ParticipantRole";

export const PARTICIPANT_ROLES = [
  "human",
  "director",
  "implementer",
  "reviewer",
  "agent",
] as const satisfies readonly ParticipantRole[];

const PARTICIPANT_ROLE_SET = new Set<string>(PARTICIPANT_ROLES);

export function isParticipantRole(value: unknown): value is ParticipantRole {
  return typeof value === "string" && PARTICIPANT_ROLE_SET.has(value);
}
