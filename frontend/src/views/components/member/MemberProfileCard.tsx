import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import ParticipantAvatarContent from "../ParticipantAvatarContent";
import ResourceImage from "../ResourceImage";
import { ROLE_OPTIONS } from "./memberHelpers";
import type { MemberEntry } from "./memberTypes";

const CARD_WIDTH = 300;
const GAP = 16;

function joinedLabel(iso?: string): string {
  if (!iso) return "";
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return "";
  return `${at.getFullYear()}년 ${at.getMonth() + 1}월 ${at.getDate()}일`;
}

// Discord's member popout: opens beside the list, level with the clicked row.
export default function MemberProfileCard({ entry, anchor, isHost, onClose }: {
  entry: MemberEntry;
  anchor: DOMRect;
  isHost: boolean;
  onClose: () => void;
}) {
  const cardRef = useRef<HTMLDivElement>(null);
  const [top, setTop] = useState(anchor.top);

  useLayoutEffect(() => {
    const height = cardRef.current?.offsetHeight || 0;
    setTop(Math.max(GAP, Math.min(anchor.top, window.innerHeight - height - GAP)));
  }, [anchor.top]);

  useEffect(() => {
    cardRef.current?.focus();
    function onPointerDown(event: PointerEvent) {
      if (!cardRef.current?.contains(event.target as Node)) onClose();
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [onClose]);

  const role = entry.owner && isHost
    ? "방장"
    : ROLE_OPTIONS.find((option) => option.id === entry.role)?.label || "사람";
  const joined = joinedLabel(entry.member?.created_at);
  const left = Math.max(GAP, anchor.left - CARD_WIDTH - GAP);

  return createPortal(
    <div ref={cardRef} className="dc-member-profile-card" role="dialog" aria-label={`${entry.displayName} 프로필`}
      tabIndex={-1} style={{ top, left, width: CARD_WIDTH }}>
      <div className="dc-member-profile-banner" aria-hidden />
      <span className="dc-member-profile-avatar" aria-hidden>
        {entry.avatarImage
          ? <ResourceImage src={entry.avatarImage} alt="" />
          : <ParticipantAvatarContent participantType={entry.member?.participant_type || "human"}
              displayName={entry.displayName} avatarLabel={entry.member?.avatar_label} size={80} />}
        <i data-active={entry.active} />
      </span>
      <div className="dc-member-profile-body">
        <h2 className="preserve-words">{entry.displayName}</h2>
        <p className="dc-member-profile-status">{entry.active ? "온라인" : "오프라인"}</p>
        <dl>
          <div><dt>역할</dt><dd>{role}</dd></div>
          {joined && <div><dt>참가한 날</dt><dd>{joined}</dd></div>}
        </dl>
      </div>
    </div>,
    document.body,
  );
}
