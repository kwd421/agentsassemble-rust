import { useEffect, useRef, useState } from "react";
import { deleteFriend, fetchSavedFriends, saveFriend } from "../api/friends";
import type { SaveFriend } from "../types/generated/SaveFriend";
import type { SavedFriend } from "../types/generated/SavedFriend";

export function useFriendsDirectory() {
  const [friends, setFriends] = useState<SavedFriend[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const active = useRef(false);
  const operation = useRef(false);
  const generation = useRef(0);

  async function run(action: () => Promise<SavedFriend[] | void>): Promise<boolean> {
    if (operation.current) return false;
    operation.current = true;
    const epoch = generation.current;
    setBusy(true); setError("");
    try {
      const next = await action();
      if (!active.current || generation.current !== epoch) return false;
      if (next) setFriends(next);
      return true;
    } catch (reason) {
      if (active.current && generation.current === epoch) {
        setError(reason instanceof Error ? reason.message : "친구 목록을 저장하지 못했어요. 다시 시도해 주세요.");
      }
      return false;
    } finally {
      if (generation.current === epoch) {
        operation.current = false;
        if (active.current) setBusy(false);
      }
    }
  }

  function reload() { return run(fetchSavedFriends); }
  useEffect(() => {
    active.current = true;
    generation.current += 1;
    operation.current = false;
    void reload();
    return () => { active.current = false; generation.current += 1; };
  }, []);

  function save(request: SaveFriend) {
    if (!friends) return Promise.resolve(false);
    return run(async () => {
      const friend = await saveFriend(request);
      return [...friends.filter((item) => item.friend_id !== friend.friend_id), friend];
    });
  }
  function remove(friendId: string) {
    if (!friends) return Promise.resolve(false);
    return run(async () => {
      await deleteFriend(friendId);
      return friends.filter((item) => item.friend_id !== friendId);
    });
  }
  return { friends: friends ?? [], loaded: friends !== null, busy, error, reload, save, remove };
}
