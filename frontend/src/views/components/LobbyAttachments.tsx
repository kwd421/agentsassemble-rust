import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { FileDown, X } from "lucide-react";
import {
  fetchMessageAttachmentBlob,
  type LobbyAttachmentRef,
  type MessageAttachmentAuthority,
} from "../../api/messageAttachments";

function formatAttachmentSize(size: number) {
  if (!Number.isFinite(size) || size <= 0) return "";
  if (size >= 1024 * 1024) return `${(size / (1024 * 1024)).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(size / 1024))} KB`;
}

export default function LobbyAttachments({
  attachments,
  roomId,
  authority,
}: {
  attachments?: LobbyAttachmentRef[];
  roomId: string;
  authority: MessageAttachmentAuthority;
}) {
  const [selectedImage, setSelectedImage] = useState<LobbyAttachmentRef | null>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const previewDialogRef = useRef<HTMLDivElement | null>(null);
  const visibleAttachments = (attachments || []).filter((attachment) => attachment.id);
  const [authorizedUrls, setAuthorizedUrls] = useState<Record<string, string>>({});
  const attachmentKey = JSON.stringify(visibleAttachments);
  const authoritySession = authority.kind === "remote" ? authority.sessionToken : "";

  useEffect(() => {
    setAuthorizedUrls({});
    setSelectedImage(null);
    openerRef.current = null;
    if (visibleAttachments.length === 0) return undefined;
    const controller = new AbortController();
    const objectUrls: string[] = [];
    let retired = false;
    const releaseObjectUrls = () => {
      objectUrls.splice(0).forEach((url) => URL.revokeObjectURL(url));
    };
    void Promise.all(
      visibleAttachments.map(async (attachment) => {
        const blob = await fetchMessageAttachmentBlob(
          attachment,
          roomId,
          authority,
          attachment.is_image ? "view" : "download",
          controller.signal,
          () => controller.signal.throwIfAborted()
        );
        controller.signal.throwIfAborted();
        const objectUrl = URL.createObjectURL(blob);
        objectUrls.push(objectUrl);
        return [attachment.id, objectUrl] as const;
      })
    ).then((entries) => {
      if (!controller.signal.aborted) setAuthorizedUrls(Object.fromEntries(entries));
    }).catch(() => {
      controller.abort();
      releaseObjectUrls();
      if (!retired) setAuthorizedUrls({});
    });
    return () => {
      retired = true;
      controller.abort();
      releaseObjectUrls();
    };
  }, [attachmentKey, authority.kind, authoritySession, roomId]);

  const closeImagePreview = useCallback(() => {
    setSelectedImage(null);
    const opener = openerRef.current;
    openerRef.current = null;
    window.setTimeout(() => opener?.focus(), 0);
  }, []);

  const openImagePreview = useCallback((attachment: LobbyAttachmentRef) => {
    openerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setSelectedImage(attachment);
  }, []);

  useEffect(() => {
    if (!selectedImage) return undefined;
    previewDialogRef.current?.querySelector<HTMLElement>("[data-preview-close]")?.focus();

    function handlePreviewKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        closeImagePreview();
        return;
      }
      if (event.key !== "Tab") return;
      const focusableElements = Array.from(
        previewDialogRef.current?.querySelectorAll<HTMLElement>("a[href], button:not([disabled])") || []
      );
      if (focusableElements.length === 0) {
        event.preventDefault();
        return;
      }
      const first = focusableElements[0];
      const last = focusableElements[focusableElements.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    window.addEventListener("keydown", handlePreviewKeyDown);
    return () => window.removeEventListener("keydown", handlePreviewKeyDown);
  }, [closeImagePreview, selectedImage]);

  if (visibleAttachments.length === 0) return null;
  const selectedUrl = selectedImage && visibleAttachments.some(
    (attachment) => attachment.id === selectedImage.id
  )
    ? authorizedUrls[selectedImage.id]
    : "";

  return (
    <>
      <div className="dc-attachment-list">
        {visibleAttachments.map((attachment) => {
          const sizeLabel = formatAttachmentSize(attachment.size);
          const authorizedUrl = authorizedUrls[attachment.id];
          if (attachment.is_image && authorizedUrl) {
            return (
              <button
                key={attachment.id}
                type="button"
                onClick={() => openImagePreview(attachment)}
                className="dc-image-attachment"
                aria-label={`${attachment.filename} 크게 보기`}
              >
                <img
                  src={authorizedUrl}
                  alt={attachment.filename}
                  loading="lazy"
                  className="dc-image-attachment-preview"
                />
              </button>
            );
          }

          return authorizedUrl ? (
            <a
              key={attachment.id}
              href={authorizedUrl}
              download={attachment.filename}
              className="dc-file-attachment"
            >
              <FileDown size={15} className="shrink-0 text-accent" />
              <span className="min-w-0 truncate preserve-words">{attachment.filename}</span>
              {sizeLabel && <span className="shrink-0 text-[10px] text-text-muted">{sizeLabel}</span>}
            </a>
          ) : (
            <span key={attachment.id} className="dc-file-attachment" aria-busy="true">
              <FileDown size={15} className="shrink-0 text-text-muted" />
              <span className="min-w-0 truncate preserve-words">{attachment.filename}</span>
            </span>
          );
        })}
      </div>

      {selectedImage && selectedUrl && createPortal(
        <div
          role="dialog"
          aria-modal="true"
          aria-label={`${selectedImage.filename} 이미지 미리보기`}
          className="dc-image-preview-overlay"
          onClick={closeImagePreview}
        >
          <div
            ref={previewDialogRef}
            className="max-h-[90vh] w-full max-w-4xl overflow-hidden rounded-xl border border-accent/24 bg-panel shadow-[0_24px_80px_rgba(0,0,0,0.55)]"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="flex items-center justify-between gap-3 border-b border-line/50 px-4 py-3">
              <p className="min-w-0 truncate text-[13px] font-black text-text-primary preserve-words">
                {selectedImage.filename}
              </p>
              <div className="flex shrink-0 items-center gap-2">
                <a
                  href={selectedUrl}
                  download={selectedImage.filename}
                  className="ops-button grid h-9 w-9 place-items-center rounded-lg"
                  aria-label={`${selectedImage.filename} 다운로드`}
                >
                  <FileDown size={16} />
                </a>
                <button
                  data-preview-close
                  type="button"
                  onClick={closeImagePreview}
                  className="ops-button grid h-9 w-9 place-items-center rounded-lg"
                  aria-label="이미지 미리보기 닫기"
                >
                  <X size={16} />
                </button>
              </div>
            </div>
            <div className="max-h-[calc(90vh-58px)] overflow-auto bg-black/32 p-3">
              <img
                src={selectedUrl}
                alt={selectedImage.filename}
                className="mx-auto max-h-[calc(90vh-90px)] max-w-full rounded-lg object-contain"
              />
            </div>
          </div>
        </div>,
        document.body
      )}
    </>
  );
}
