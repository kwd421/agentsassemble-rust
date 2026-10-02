import type { CSSProperties } from "react";

export type RoomAppearance = {
  bannerPreset: "default" | "forest" | "midnight" | "ember" | "custom";
  bannerImage?: string;
  iconImage?: string;
  iconLabel?: string;
  notifications: "all" | "mentions" | "mute";
  inviteScope: "room" | "read_only";
};

export const DEFAULT_ROOM_APPEARANCE: RoomAppearance = {
  bannerPreset: "default",
  notifications: "mentions",
  inviteScope: "room",
};

export function roomAppearanceStyle(appearance: RoomAppearance): CSSProperties {
  return {
    "--room-banner-image": appearance.bannerImage
      ? `url("${appearance.bannerImage}")`
      : "none",
    "--room-icon-image": appearance.iconImage ? `url("${appearance.iconImage}")` : "none",
  } as CSSProperties;
}

export function completeRoomAppearance(
  appearance: Partial<RoomAppearance> | undefined
): RoomAppearance {
  return {
    ...DEFAULT_ROOM_APPEARANCE,
    ...(appearance || {}),
  };
}

// A room without an icon image shows its initials, as Discord does for servers:
// the first character of each word, at most three.
export function roomInitials(label: string): string {
  const initials = label
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map((word) => Array.from(word)[0])
    .slice(0, 3)
    .join("");
  return initials || "?";
}
