import { useEffect, useRef, useState } from "react";
import { deleteFriend, fetchSavedFriends, saveFriend } from "../api/friends";
import type { SaveFriend } from "../types/generated/SaveFriend";
import type { SavedFriend } from "../types/generated/SavedFriend";

import type { RoomHttpAuthority } from "../api/roomHttpAuthority";

export function useFriendsDirectory(authority: RoomHttpAuthority = { kind: "local" }) {
  const scope = JSON.stringify(authority);
  const [loadedScope, setLoadedScope] = useState("");
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
      if (next) { setFriends(next); setLoadedScope(scope); }
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

  function reload() { return run(() => fetchSavedFriends(authority)); }
  useEffect(() => {
    active.current = true;
    generation.current += 1;
    operation.current = false;
    void reload();
    return () => { active.current = false; generation.current += 1; };
  }, [scope]);

  function save(request: SaveFriend) {
    if (!friends || loadedScope !== scope) return Promise.resolve(false);
    return run(async () => {
      const friend = await saveFriend(request, authority);
      return [...friends.filter((item) => item.friend_id !== friend.friend_id), friend];
    });
  }
  function remove(friendId: string) {
    if (!friends || loadedScope !== scope) return Promise.resolve(false);
    return run(async () => {
      await deleteFriend(friendId, authority);
      return friends.filter((item) => item.friend_id !== friendId);
    });
  }
  return { friends: loadedScope === scope ? friends ?? [] : [], loaded: loadedScope === scope && friends !== null, busy, error, reload, save, remove };
}
