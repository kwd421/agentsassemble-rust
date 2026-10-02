import type { useFriendsDirectory } from "../../app/useFriendsDirectory";
import { useState } from "react";

// Only saved human friends can receive a named invite; without any, the general
// link is the whole choice and the picker stays out of the way.
export default function SavedFriendInvitePicker({ onSelect, directory }: { onSelect: (name?: string) => void; directory: ReturnType<typeof useFriendsDirectory> }) {
  const [selectedId, setSelectedId] = useState("");
  const people = directory.friends.filter((friend) => friend.details.participant_type === "human");
  if (!people.length && !directory.error) return null;
  return <section className="dc-invite-options" style={{ gridTemplateColumns: "1fr" }}>
    {people.length > 0 && <label>
      <span>받는 사람</span>
      <select style={{ minHeight: 44, maxWidth: "100%", appearance: "none" }} disabled={directory.busy || !directory.loaded}
        value={selectedId} onChange={(event) => {
          const id = event.target.value;
          const friend = people.find((person) => person.friend_id === id);
          if (id && !friend) return;
          setSelectedId(id); onSelect(friend?.details.display_name);
        }}>
        <option value="">누구나</option>
        {people.map((friend) => <option key={friend.friend_id} value={friend.friend_id}>{[friend.details.display_name, friend.details.handle].filter(Boolean).join(" · ")}</option>)}
      </select>
    </label>}
    {directory.error && <div role="alert" className="dc-invite-inline-error">{directory.error}<button type="button" className="dc-invite-row-button" style={{ minHeight: 44, marginLeft: 12 }} disabled={directory.busy} onClick={() => void directory.reload()}>다시 읽기</button></div>}
  </section>;
}
