import type { SaveFriend } from "../types/generated/SaveFriend";
import type { SavedFriend } from "../types/generated/SavedFriend";
import type { RoomHttpAuthority } from "./roomHttpAuthority";
import { fetchDesktopOperatorRuntime } from "../lib/desktopBridge";
import { responseError } from "./http";

async function request<T>(authority: RoomHttpAuthority, method: string, body?: object, query = ""): Promise<T> {
  const init: RequestInit = {
    method, cache: "no-store", credentials: "omit", redirect: "error", referrerPolicy: "no-referrer",
    headers: { ...(body ? { "Content-Type": "application/json" } : {}),
      ...(authority.kind === "remote" ? { Authorization: `Bearer ${authority.sessionToken}`,
        "X-Device-Token": authority.deviceToken || "" } : {}) },
    ...(body ? { body: JSON.stringify(body) } : {}),
  };
  const response = authority.kind === "local"
    ? await fetchDesktopOperatorRuntime(`/api/room-friends${query}`, init)
    : await fetch(`/api/central-owner/friends${query}`, init);
  if (!response.ok) throw await responseError(response);
  return response.json();
}

export async function fetchSavedFriends(authority: RoomHttpAuthority = { kind: "local" }): Promise<SavedFriend[]> {
  return (await request<{ friends: SavedFriend[] }>(authority, "GET")).friends;
}

export function saveFriend(body: SaveFriend, authority: RoomHttpAuthority = { kind: "local" }): Promise<SavedFriend> {
  return request(authority, "POST", body);
}

export function deleteFriend(friendId: string, authority: RoomHttpAuthority = { kind: "local" }): Promise<{ deleted: boolean }> {
  return request(authority, "DELETE", undefined, `?friend_id=${encodeURIComponent(friendId)}`);
}
