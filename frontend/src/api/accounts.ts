import type { UserProfileIdentity } from "./userProfile";
import { responseError } from "./http";
import { fetchDesktopOperatorRuntime, isDesktopWebview } from "../lib/desktopBridge";

export type PublicAccount = {
  account_id: string;
  provider: "google";
  display_name: string;
  email: string;
  avatar_image_url: string;
};
export type AccountStatus = {
  account: PublicAccount | null;
  google: { enabled: boolean; client_id: string; unavailable_reason: string };
};
export type GoogleChallenge = { status: "ready"; client_id: string; nonce: string };
export type GoogleConnection = {
  status: "connected";
  identity_switched: boolean;
  account: PublicAccount;
  user: { user_id: string; participant_id: string; display_name: string; avatar_image_url: string };
};

async function accountRequest<T>(path: string, identity: UserProfileIdentity, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  let response: Response;
  if (!identity.sessionToken && isDesktopWebview()) {
    response = await fetchDesktopOperatorRuntime(path, { ...init, cache: "no-store", headers });
  } else {
    if (identity.sessionToken) headers.set("Authorization", `Bearer ${identity.sessionToken}`);
    if (identity.deviceToken) headers.set("X-Device-Token", identity.deviceToken);
    response = await fetch(path, { ...init, cache: "no-store", headers });
  }
  if (!response.ok) throw await responseError(response);
  return response.json() as Promise<T>;
}
export function fetchAccountStatus(identity: UserProfileIdentity): Promise<AccountStatus> {
  return accountRequest("/api/account", identity);
}
export function startGoogleAccountLogin(identity: UserProfileIdentity): Promise<GoogleChallenge> {
  return accountRequest("/api/account/google/challenge", identity, {
    method: "POST", headers: { "Content-Type": "application/json" }, body: "{}",
  });
}
export function connectGoogleAccount(identity: UserProfileIdentity, credential: string, nonce: string): Promise<GoogleConnection> {
  return accountRequest("/api/account/google", identity, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ credential, nonce, discard_guest_on_account_switch: true }),
  });
}
export function disconnectGoogleAccount(identity: UserProfileIdentity): Promise<{ status: "disconnected" }> {
  return accountRequest("/api/account/google", identity, { method: "DELETE" });
}
