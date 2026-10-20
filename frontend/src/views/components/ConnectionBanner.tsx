import { X } from "../../ui/icons";

export default function ConnectionBanner({ message, onDismiss }: { message: string; onDismiss?: () => void }) {
  return (
    <div role="status" className="dc-connection-banner" data-dismissible={onDismiss ? "true" : undefined}>
      <span aria-hidden>●</span> {message}
      {onDismiss && (
        <button type="button" className="dc-connection-banner-close" aria-label="알림 닫기" onClick={onDismiss}>
          <X size={16} aria-hidden />
        </button>
      )}
    </div>
  );
}
