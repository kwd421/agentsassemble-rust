import { UserX } from "lucide-react";
import type { RoomDockItem } from "../../lib/roomDockModel";
import type { MembershipEndedScope } from "../../lib/memberMembershipEnded";
import { membershipEndedCopy } from "../../lib/memberMembershipEnded";

export default function MembershipEndedView({ room, scope, onConfirm }: {
  room: RoomDockItem;
  scope: MembershipEndedScope;
  onConfirm: () => void;
}) {
  const copy = membershipEndedCopy(room.label, scope);
  return (
    <section className="dc-disconnected-room" aria-labelledby="membership-ended-title">
      <div className="dc-disconnected-room-icon" aria-hidden>
        <UserX size={30} />
      </div>
      <h1 id="membership-ended-title">{copy.title}</h1>
      <p className="dc-disconnected-room-name">{room.label}</p>
      <p className="dc-disconnected-room-description">{copy.body}</p>
      <button type="button" className="ops-button min-h-11 px-4" onClick={onConfirm}>확인</button>
    </section>
  );
}
