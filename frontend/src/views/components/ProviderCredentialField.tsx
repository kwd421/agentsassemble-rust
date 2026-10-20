import type { ReactNode } from "react";
import type { ProviderCredentialStatus } from "../../api";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";

type ProviderCredentialFieldProps = {
  provider: NativeCliProviderAvailability;
  status: ProviderCredentialStatus | null;
  value: string;
  busy: boolean;
  onValueChange: (value: string) => void;
  onSave: () => void;
  onDelete: () => void;
  /** Provider-owned non-secret settings that belong beside the API key. */
  children?: ReactNode;
};

export default function ProviderCredentialField({
  provider,
  status,
  value,
  busy,
  onValueChange,
  onSave,
  onDelete,
  children,
}: ProviderCredentialFieldProps) {
  return (
    <section className="dc-agent-section">
      <p className="dc-agent-section-title">인증</p>
      <div className="dc-provider-secret-field">
        <label className="dc-agent-field">
          <span>API 키</span>
          <div className="dc-provider-secret-row">
            <input className="dc-input"
              type="password"
              autoComplete="off"
              value={value}
              placeholder={
                status?.configured ? "저장된 키 사용 중" : `${provider.display_name} API 키`
              }
              onChange={(event) => onValueChange(event.currentTarget.value)}
            />
            <button
              type="button"
              style={{ minWidth: 96 }}
              disabled={!value.trim() || busy}
              onClick={onSave}
            >
              저장
            </button>
            {status?.source === "keyring" && (
              <button type="button" data-variant="danger" disabled={busy} onClick={onDelete}>
                키 삭제
              </button>
            )}
          </div>
        </label>
      </div>
      {children}
    </section>
  );
}
