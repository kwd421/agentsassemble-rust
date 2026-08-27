import {
  fetchDesktopOperatorRuntimeWithBase,
  isDesktopWebview,
} from "../lib/desktopBridge";
import { profileAvatarReference } from "../lib/attachmentReference";
import {
  fetchJsonWithIdentity,
  postJsonWithIdentity,
  responseError,
} from "./http";

export interface UserProfile {
  displayName: string;
  handle: string;
  status: "online" | "idle" | "dnd" | "offline";
  customStatus: string;
  avatarLabel: string;
  avatarImage?: string;
  bannerPreset: "default" | "forest" | "midnight" | "ember" | "custom";
  accentColor: string;
  micMuted: boolean;
  deafened: boolean;
  createdAt?: string;
  updatedAt?: string;
}

export type UserProfileIdentity = {
  sessionToken?: string;
  deviceToken?: string;
  roomId?: string;
};

export type UserProfileSnapshot = {
  profile: UserProfile;
  displayResourceBase: string;
};

type ApiUserProfile = {
  revision?: number;
  display_name?: string;
  handle?: string;
  status?: UserProfile["status"];
  custom_status?: string;
  avatar_label?: string;
  avatar_image_url?: string;
  banner_preset?: UserProfile["bannerPreset"];
  accent_color?: string;
  mic_muted?: boolean;
  deafened?: boolean;
  created_at?: string;
  updated_at?: string;
};

function normalizeUserProfile(payload: ApiUserProfile | undefined): UserProfile {
  if (
    !payload ||
    !Number.isInteger(payload.revision) ||
    Number(payload.revision) < 1 ||
    typeof payload.display_name !== "string" ||
    typeof payload.handle !== "string" ||
    !["online", "idle", "dnd", "offline"].includes(String(payload.status || "")) ||
    typeof payload.custom_status !== "string" ||
    typeof payload.avatar_label !== "string" ||
    typeof payload.avatar_image_url !== "string" ||
    !["default", "forest", "midnight", "ember", "custom"].includes(
      String(payload.banner_preset || "")
    ) ||
    typeof payload.accent_color !== "string" ||
    typeof payload.mic_muted !== "boolean" ||
    typeof payload.deafened !== "boolean" ||
    typeof payload.created_at !== "string" ||
    typeof payload.updated_at !== "string"
  ) {
    throw new Error("서버 사용자 프로필 응답이 현재 계약과 일치하지 않습니다.");
  }
  return {
    displayName: payload.display_name,
    handle: payload.handle,
    status: payload.status as UserProfile["status"],
    customStatus: payload.custom_status,
    avatarLabel: payload.avatar_label,
    avatarImage: profileAvatarReference(payload.avatar_image_url),
    bannerPreset: payload.banner_preset as UserProfile["bannerPreset"],
    accentColor: payload.accent_color,
    micMuted: payload.mic_muted,
    deafened: payload.deafened,
    createdAt: payload.created_at,
    updatedAt: payload.updated_at,
  };
}

function userProfileToApi(profile: UserProfile): ApiUserProfile {
  return {
    display_name: profile.displayName,
    handle: profile.handle,
    status: profile.status,
    custom_status: profile.customStatus,
    avatar_label: profile.avatarLabel,
    avatar_image_url: profileAvatarReference(profile.avatarImage),
    banner_preset: profile.bannerPreset,
    accent_color: profile.accentColor,
    mic_muted: profile.micMuted,
    deafened: profile.deafened,
  };
}

function browserDisplayResourceBase(): string {
  const endpoint = new URL("/api/user-profile", window.location.href);
  if (!["http:", "https:"].includes(endpoint.protocol)) {
    throw new Error("프로필 표시 자원 출처가 안전하지 않습니다.");
  }
  return endpoint.origin;
}

async function requestDesktopProfile(
  init: RequestInit
): Promise<{ payload: { profile: ApiUserProfile }; displayResourceBase: string }> {
  const result = await fetchDesktopOperatorRuntimeWithBase("/api/user-profile", init);
  if (!result.response.ok) throw await responseError(result.response);
  return {
    payload: (await result.response.json()) as { profile: ApiUserProfile },
    displayResourceBase: result.httpBaseUrl,
  };
}

export async function fetchUserProfile(
  identity: UserProfileIdentity = {}
): Promise<UserProfileSnapshot> {
  if (!identity.sessionToken && isDesktopWebview()) {
    const result = await requestDesktopProfile({ cache: "no-store" });
    return {
      profile: normalizeUserProfile(result.payload.profile),
      displayResourceBase: result.displayResourceBase,
    };
  }
  const displayResourceBase = browserDisplayResourceBase();
  const payload = await fetchJsonWithIdentity<{ profile: ApiUserProfile }>(
    "/api/user-profile",
    identity
  );
  return {
    profile: normalizeUserProfile(payload.profile),
    displayResourceBase,
  };
}

export async function saveUserProfile(
  profile: UserProfile,
  identity: UserProfileIdentity = {}
): Promise<UserProfileSnapshot> {
  const body = userProfileToApi(profile);
  if (!identity.sessionToken && isDesktopWebview()) {
    const result = await requestDesktopProfile({
      cache: "no-store",
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return {
      profile: normalizeUserProfile(result.payload.profile),
      displayResourceBase: result.displayResourceBase,
    };
  }
  const displayResourceBase = browserDisplayResourceBase();
  const payload = await postJsonWithIdentity<{ profile: ApiUserProfile }>(
    "/api/user-profile",
    body,
    identity
  );
  return {
    profile: normalizeUserProfile(payload.profile),
    displayResourceBase,
  };
}
