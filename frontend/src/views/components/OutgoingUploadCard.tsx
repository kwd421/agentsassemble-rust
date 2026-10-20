import { FileUp, RotateCcw, X } from "../../ui/icons";

function megabytes(bytes: number) {
  const value = bytes / (1024 * 1024);
  return `${value >= 10 ? Math.round(value) : value.toFixed(1)}MB`;
}

export type OutgoingUploadPhase = "uploading" | "sending" | "failed" | "unknown";

// One card for a whole outgoing message: its files share a single progress bar,
// as in Discord, instead of one row per file.
export default function OutgoingUploadCard({ phase, fileCount, bytesDone, bytesTotal, error, onCancel, onRetry, onDiscard }: {
  phase: OutgoingUploadPhase;
  fileCount: number;
  bytesDone: number;
  bytesTotal: number;
  error?: string;
  onCancel?: () => void;
  onRetry?: () => void;
  onDiscard?: () => void;
}) {
  const ratio = bytesTotal > 0 ? Math.min(1, bytesDone / bytesTotal) : 0;
  const stopped = phase === "failed" || phase === "unknown";
  const title = phase === "failed" ? "올리지 못했어요"
    : phase === "unknown" ? "보냈는지 확인하지 못했어요"
    : phase === "sending" ? "보내는 중"
    : `파일 ${fileCount}개 올리는 중`;
  const detail = stopped ? error
    : phase === "sending" ? undefined
    : `${megabytes(bytesDone)} / ${megabytes(bytesTotal)}`;
  return (
    <div className="dc-outgoing-upload" data-phase={phase} role="status" aria-live="polite">
      <FileUp size={28} className="dc-outgoing-upload-icon" aria-hidden="true" />
      <div className="dc-outgoing-upload-main">
        <p className="dc-outgoing-upload-title">
          <span className="truncate">{title}</span>
          {detail && <span className="dc-outgoing-upload-detail truncate">{detail}</span>}
        </p>
        {!stopped && (
          <div className="dc-outgoing-upload-bar" role="progressbar" aria-label="올리기 진행률"
            aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(ratio * 100)}>
            <span style={{ width: `${phase === "sending" ? 100 : ratio * 100}%` }} />
          </div>
        )}
      </div>
      {stopped && onRetry && (
        <button type="button" className="dc-button dc-button--secondary" onClick={onRetry}>
          <RotateCcw size={14} aria-hidden="true" />
          다시 시도
        </button>
      )}
      {(stopped ? onDiscard : onCancel) && (
        <button type="button" className="dc-outgoing-upload-close" onClick={stopped ? onDiscard : onCancel}
          aria-label={stopped ? "지우기" : "올리기 취소"} data-tip={stopped ? "지우기" : "취소"}>
          <X size={18} />
        </button>
      )}
    </div>
  );
}
