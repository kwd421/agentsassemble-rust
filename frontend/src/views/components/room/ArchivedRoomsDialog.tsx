import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import ArchivedRoomList from "./ArchivedRoomList";
import type { RoomLifecycleController } from "./RoomLifecycleConfirm";

/**
 * Room settings holds the same list, but a room has to exist to open them. This
 * dialog is the way back when every room is archived and the rail is empty.
 */
export default function ArchivedRoomsDialog({
  controller,
  onClose,
}: {
  controller: RoomLifecycleController;
  onClose: () => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => { dialog?.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [opener]);

  return (
    <div className="dc-modal-backdrop" role="presentation" onClick={onClose}>
      <dialog
        ref={dialogRef}
        className="dc-invite-modal"
        style={{ margin: "auto", padding: 24, width: "min(560px, calc(100vw - 32px))", maxHeight: "calc(100dvh - 32px)", color: "var(--color-text-primary)" }}
        aria-modal="true"
        aria-label="방 관리"
        onCancel={(event) => { event.preventDefault(); onClose(); }}
        onClick={(event) => { event.stopPropagation(); if (event.target === event.currentTarget) onClose(); }}
      >
        <header className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h2 className="text-[18px] font-black text-text-primary">방 관리</h2>
            <p className="mt-1 text-[13px] text-text-muted preserve-words">
              보관하거나 종료한 방은 서버 목록에 나오지 않아요.
            </p>
          </div>
          <button type="button" className="dc-modal-close" style={{ minWidth: 44, minHeight: 44, flexShrink: 0 }}
            onClick={onClose} aria-label="방 관리 닫기">
            <X size={18} />
          </button>
        </header>
        <ArchivedRoomList controller={controller} />
      </dialog>
    </div>
  );
}
