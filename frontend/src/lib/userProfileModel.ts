import type { CSSProperties } from "react";
import type { UserProfile } from "../api";

export const DEFAULT_USER_PROFILE: UserProfile = {
  displayName: "SeiNel",
  handle: "seinel.",
  status: "online",
  customStatus: "AgentsAssemble",
  avatarLabel: "나",
  avatarImage: undefined,
  bannerPreset: "default",
  accentColor: "#5865f2",
  micMuted: true,
  deafened: false,
};

export const PROFILE_STATUS_OPTIONS: Array<{
  id: UserProfile["status"];
  label: string;
  helper: string;
}> = [
  { id: "online", label: "온라인", helper: "" },
  { id: "idle", label: "자리 비움", helper: "" },
  { id: "dnd", label: "방해 금지", helper: "" },
  { id: "offline", label: "오프라인 표시", helper: "다른 사람에게 오프라인으로 보여요." },
];

export function profileStatusClass(profile: UserProfile, hasBackendError: boolean) {
  if (hasBackendError || profile.status === "offline") return "offline";
  if (profile.status === "idle") return "idle";
  if (profile.status === "dnd") return "dnd";
  return "online";
}

export function profileStatusLabel(status: UserProfile["status"]) {
  return PROFILE_STATUS_OPTIONS.find((option) => option.id === status)?.label || "온라인";
}

export function profileCssVars(
  profile: UserProfile,
  avatarUrl: string | undefined
): CSSProperties {
  return {
    "--profile-accent": profile.accentColor,
    "--profile-avatar-image": avatarUrl ? `url("${avatarUrl}")` : undefined,
  } as CSSProperties;
}
