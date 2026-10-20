import { useEffect, useId, useRef, useState } from "react";

// Shown when a selection has more files than one message can carry. Nothing is
// attached; the person picks again.
export default function UploadLimitDialog({ limit, onClose }: { limit: number; onClose: () => void }) {
  const titleId = useId();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);

  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => {
      dialog?.close();
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [opener]);

  return (
    <dialog
      ref={dialogRef}
      className="dc-dialog dc-upload-limit-dialog"
      aria-labelledby={titleId}
      onCancel={(event) => { event.preventDefault(); onClose(); }}
      onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}
    >
      <h2 id={titleId} className="dc-dialog-title">파일이 너무 많아요</h2>
      <p>한 번에 {limit}개까지 올릴 수 있어요.</p>
      <button autoFocus type="button" className="dc-button dc-button--primary" onClick={onClose}>
        확인
      </button>
    </dialog>
  );
}
