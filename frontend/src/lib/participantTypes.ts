import { Bot, User } from "lucide-react";
import type { LucideIcon } from "lucide-react";

export type ParticipantTypeMeta = {
  label: string;
  detail: string;
  icon: LucideIcon;
  tone: string;
};

export function participantTypeMeta(type: string): ParticipantTypeMeta {
  if (type === "human") {
    return { label: "사람", detail: "브라우저/초대 사용자", icon: User, tone: "human" };
  }
  if (type === "agent") {
    return { label: "에이전트", detail: "Agent Session", icon: Bot, tone: "subscription" };
  }
  throw new Error("Room participant has an unsupported canonical type.");
}
