import { useFriendsDirectory } from "../../app/useFriendsDirectory";
import { useState } from "react";

export default function SavedFriendInvitePicker({ onSelect }: { onSelect: (name?: string) => void }) {
  const directory = useFriendsDirectory();
  const [selectedId, setSelectedId] = useState("");
  const people = directory.friends.filter((friend) => friend.details.participant_type === "human");
  return <section style={{ display: "grid", gap: 12 }}>
    <label style={{ display: "grid", gap: 8 }}>사람 친구 초대
      <select className="ops-input" style={{ minHeight: 44, maxWidth: "100%" }} disabled={directory.busy || !directory.loaded}
        value={selectedId} onChange={(event) => {
          const id = event.target.value;
          const friend = people.find((person) => person.friend_id === id);
          if (id && !friend) return;
          setSelectedId(id); onSelect(friend?.details.display_name);
        }}>
        <option value="">일반 초대</option>
        {people.map((friend) => <option key={friend.friend_id} value={friend.friend_id}>{[friend.details.display_name, friend.details.handle].filter(Boolean).join(" · ")}</option>)}
      </select>
    </label>
    {directory.error && <div role="alert">{directory.error}<button type="button" className="ops-button" style={{ minHeight: 44, marginLeft: 12 }} disabled={directory.busy} onClick={() => void directory.reload()}>다시 읽기</button></div>}
  </section>;
}
