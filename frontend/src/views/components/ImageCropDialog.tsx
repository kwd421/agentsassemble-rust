import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import ImageCropper, { type ImageCropShape } from "./ImageCropper";

/**
 * The modal "이미지 편집" step shared by every image the user picks for an identity:
 * profile photos, room icons and room banners. Choosing the file happens before this
 * opens, straight from the trigger button, so the dialog only ever crops.
 */
export default function ImageCropDialog({
  title,
  file,
  shape,
  busy = false,
  status = "",
  onCancel,
  onApply,
}: {
  title: string;
  file: File;
  shape: ImageCropShape;
  busy?: boolean;
  status?: string;
  onCancel: () => void;
  onApply: (file: File) => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  return (
    <dialog
      ref={dialogRef}
      className="dc-image-crop-dialog"
      aria-label={title}
      onKeyDown={(event) => { if (event.key === "Escape") event.stopPropagation(); }}
      onCancel={(event) => { event.preventDefault(); if (!busy) onCancel(); }}
      onClick={(event) => { event.stopPropagation(); if (event.target === event.currentTarget && !busy) onCancel(); }}
    >
      <header className="dc-image-crop-dialog-head">
        <h2>{title}</h2>
        <button type="button" className="dc-modal-close" style={{ minWidth: 44, minHeight: 44 }}
          aria-label={`${title} 닫기`} disabled={busy} onClick={onCancel}>
          <X size={18} />
        </button>
      </header>
      <fieldset className="dc-image-crop-dialog-body" disabled={busy} style={{ border: 0, margin: 0, minWidth: 0 }}>
        <ImageCropper file={file} shape={shape} onCancel={onCancel} onCropped={onApply} />
        {status && <p className="dc-image-crop-dialog-status" role="status">{status}</p>}
      </fieldset>
    </dialog>
  );
}
