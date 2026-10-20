import { UserX } from "../../ui/icons";
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
      <p className="dc-disconnected-room-description">{copy.body}</p>
      <button type="button" className="dc-button dc-button--secondary dc-button--lg min-h-10 min-w-[96px] px-4" onClick={onConfirm}>확인</button>
    </section>
  );
}
