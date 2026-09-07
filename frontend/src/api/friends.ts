import type { SaveFriend } from "../types/generated/SaveFriend";
import type { SavedFriend } from "../types/generated/SavedFriend";
import { deleteJsonServerOperator, fetchJsonServerOperator, postJsonServerOperator } from "./http";

export async function fetchSavedFriends(): Promise<SavedFriend[]> {
  const response = await fetchJsonServerOperator<{ friends: SavedFriend[] }>("/api/room-friends");
  return response.friends;
}

export function saveFriend(request: SaveFriend): Promise<SavedFriend> {
  return postJsonServerOperator("/api/room-friends", request);
}

export function deleteFriend(friendId: string): Promise<{ deleted: boolean }> {
  return deleteJsonServerOperator(`/api/room-friends?friend_id=${encodeURIComponent(friendId)}`);
}
