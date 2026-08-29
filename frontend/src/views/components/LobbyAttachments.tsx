import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { FileDown, X } from "lucide-react";
import type { LobbyAttachmentRef } from "../../api/messageAttachments";
import type { MessageAttachmentReadScheduler } from "../../lib/messageAttachmentReadScheduler";

function formatAttachmentSize(size: number) {
  if (!Number.isFinite(size) || size <= 0) return "";
  if (size >= 1024 * 1024) return `${(size / (1024 * 1024)).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(size / 1024))} KB`;
}

function LobbyFileAttachment({
  attachment,
  scheduler,
}: {
  attachment: LobbyAttachmentRef;
  scheduler: MessageAttachmentReadScheduler;
}) {
  const activeRead = useRef<AbortController | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const sizeLabel = formatAttachmentSize(attachment.size);

  useLayoutEffect(() => () => {
    activeRead.current?.abort();
    activeRead.current = null;
  }, [attachment.id, scheduler]);

  async function download() {
    if (loading) return;
    const controller = new AbortController();
    activeRead.current?.abort();
    activeRead.current = controller;
    setLoading(true);
    setError("");
    try {
      const blob = await scheduler.read(attachment, "download", controller.signal);
      controller.signal.throwIfAborted();
      const objectUrl = URL.createObjectURL(blob);
      try {
        const anchor = document.createElement("a");
        anchor.href = objectUrl;
        anchor.download = attachment.filename;
        anchor.click();
      } finally {
        URL.revokeObjectURL(objectUrl);
      }
    } catch (errorValue) {
      if (!controller.signal.aborted && activeRead.current === controller) {
        setError(errorValue instanceof Error ? errorValue.message : "첨부 다운로드 실패");
      }
    } finally {
      if (activeRead.current === controller) {
        activeRead.current = null;
        setLoading(false);
      }
    }
  }

  return (
    <button
      type="button"
      onClick={download}
      disabled={loading}
      className="dc-file-attachment"
      aria-busy={loading}
      aria-label={`${attachment.filename} ${error ? "다운로드 다시 시도" : "다운로드"}`}
      title={error || undefined}
    >
      <FileDown size={15} className="shrink-0 text-accent" />
      <span className="min-w-0 truncate preserve-words">{attachment.filename}</span>
      {sizeLabel && <span className="shrink-0 text-[10px] text-text-muted">{sizeLabel}</span>}
    </button>
  );
}

function LobbyImageAttachment({
  attachment,
  scheduler,
}: {
  attachment: LobbyAttachmentRef;
  scheduler: MessageAttachmentReadScheduler;
}) {
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const previewDialogRef = useRef<HTMLDivElement | null>(null);
  const [intersecting, setIntersecting] = useState(false);
  const [objectUrl, setObjectUrl] = useState("");
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const element = triggerRef.current;
    if (!element || typeof IntersectionObserver !== "function") {
      setError("이미지 미리보기 관찰 경계를 사용할 수 없습니다.");
      return undefined;
    }
    const observer = new IntersectionObserver((entries) => {
      setIntersecting(entries.some((entry) => entry.isIntersecting));
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [attachment.id]);

  useLayoutEffect(() => {
    setObjectUrl("");
    setOpen(false);
    if (!intersecting) return undefined;
    const controller = new AbortController();
    let createdUrl = "";
    setError("");
    void scheduler.read(attachment, "view", controller.signal).then(
      (blob) => {
        controller.signal.throwIfAborted();
        createdUrl = URL.createObjectURL(blob);
        setObjectUrl(createdUrl);
      },
      (errorValue) => {
        if (!controller.signal.aborted) {
          setError(errorValue instanceof Error ? errorValue.message : "이미지 미리보기 실패");
        }
      }
    );
    return () => {
      controller.abort();
      if (createdUrl) URL.revokeObjectURL(createdUrl);
    };
  }, [attachment.id, intersecting, retry, scheduler]);

  const closePreview = useCallback(() => {
    setOpen(false);
    window.setTimeout(() => triggerRef.current?.focus(), 0);
  }, []);

  useEffect(() => {
    if (!open || !objectUrl) return undefined;
    previewDialogRef.current?.querySelector<HTMLElement>("[data-preview-close]")?.focus();
    function handlePreviewKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        closePreview();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = Array.from(
        previewDialogRef.current?.querySelectorAll<HTMLElement>("a[href], button:not([disabled])") || []
      );
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (!first || !last) {
        event.preventDefault();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
    window.addEventListener("keydown", handlePreviewKeyDown);
    return () => window.removeEventListener("keydown", handlePreviewKeyDown);
  }, [closePreview, objectUrl, open]);

  const label = objectUrl
    ? `${attachment.filename} 크게 보기`
    : error
      ? `${attachment.filename} 미리보기 다시 시도`
      : intersecting
        ? `${attachment.filename} 불러오는 중`
        : `${attachment.filename} 미리보기 대기`;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        disabled={!objectUrl && !error}
        onClick={() => objectUrl ? setOpen(true) : setRetry((value) => value + 1)}
        className={objectUrl ? "dc-image-attachment" : "dc-file-attachment"}
        aria-label={label}
        aria-busy={intersecting && !objectUrl && !error}
        title={error || undefined}
      >
        {objectUrl ? (
          <img
            src={objectUrl}
            alt={attachment.filename}
            className="dc-image-attachment-preview"
          />
        ) : (
          <>
            <FileDown size={15} className="shrink-0 text-text-muted" />
            <span className="min-w-0 truncate preserve-words">{attachment.filename}</span>
          </>
        )}
      </button>

      {open && objectUrl && createPortal(
        <div
          role="dialog"
          aria-modal="true"
          aria-label={`${attachment.filename} 이미지 미리보기`}
          className="dc-image-preview-overlay"
          onClick={closePreview}
        >
          <div
            ref={previewDialogRef}
            className="max-h-[90vh] w-full max-w-4xl overflow-hidden rounded-xl border border-accent/24 bg-panel shadow-[0_24px_80px_rgba(0,0,0,0.55)]"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="flex items-center justify-between gap-3 border-b border-line/50 px-4 py-3">
              <p className="min-w-0 truncate text-[13px] font-black text-text-primary preserve-words">
                {attachment.filename}
              </p>
              <div className="flex shrink-0 items-center gap-2">
                <a
                  href={objectUrl}
                  download={attachment.filename}
                  className="ops-button grid h-9 w-9 place-items-center rounded-lg"
                  aria-label={`${attachment.filename} 다운로드`}
                >
                  <FileDown size={16} />
                </a>
                <button
                  data-preview-close
                  type="button"
                  onClick={closePreview}
                  className="ops-button grid h-9 w-9 place-items-center rounded-lg"
                  aria-label="이미지 미리보기 닫기"
                >
                  <X size={16} />
                </button>
              </div>
            </div>
            <div className="max-h-[calc(90vh-58px)] overflow-auto bg-black/32 p-3">
              <img
                src={objectUrl}
                alt={attachment.filename}
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

export default function LobbyAttachments({
  attachments,
  scheduler,
}: {
  attachments?: LobbyAttachmentRef[];
  scheduler: MessageAttachmentReadScheduler;
}) {
  const visibleAttachments = (attachments || []).filter((attachment) => attachment.id);
  if (visibleAttachments.length === 0) return null;

  return (
    <div className="dc-attachment-list">
      {visibleAttachments.map((attachment) =>
        attachment.is_image ? (
          <LobbyImageAttachment
            key={attachment.id}
            attachment={attachment}
            scheduler={scheduler}
          />
        ) : (
          <LobbyFileAttachment
            key={attachment.id}
            attachment={attachment}
            scheduler={scheduler}
          />
        )
      )}
    </div>
  );
}
