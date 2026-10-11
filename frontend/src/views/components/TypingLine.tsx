import { Fragment } from "react";
import type { RoomTypingIndicator } from "../../lib/roomTypingIndicators";

// One quiet line right above the composer. It floats over the list's bottom
// padding, so messages do not jump when someone starts or stops writing.
export default function TypingLine({ indicators }: { indicators: RoomTypingIndicator[] }) {
  if (indicators.length === 0) return null;
  const compacting = indicators.every((indicator) => indicator.activity === "compacting");
  return <div className="dc-typing-line" role="status" aria-live="polite">
    <span className="dc-typing-dots" aria-hidden="true"><span /><span /><span /></span>
    <span className="min-w-0 truncate">
      {indicators.length > 3 ? "여러 명이" : <>{indicators.map((indicator, index) => <Fragment key={indicator.participantId || indicator.displayName}>
        {index > 0 && ", "}<strong>{indicator.displayName}</strong>
      </Fragment>)} 님이</>} {compacting ? "대화를 압축하고 있어요…" : "입력하고 있어요…"}
    </span>
  </div>;
}
