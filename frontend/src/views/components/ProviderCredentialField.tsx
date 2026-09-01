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
};

export default function ProviderCredentialField({
  provider,
  status,
  value,
  busy,
  onValueChange,
  onSave,
  onDelete,
}: ProviderCredentialFieldProps) {
  return (
    <section className="dc-agent-section">
      <p className="dc-agent-section-title">인증</p>
      <div className="dc-provider-secret-field">
        <label className="dc-agent-field">
          <span>API 키</span>
          <input
            type="password"
            autoComplete="off"
            value={value}
            placeholder={
              status?.configured ? "설정됨" : `${provider.display_name} API key`
            }
            onChange={(event) => onValueChange(event.currentTarget.value)}
          />
        </label>
        <div>
          <button
            type="button"
            disabled={!value.trim() || busy}
            onClick={onSave}
          >
            보안 저장
          </button>
          {status?.source === "keyring" && (
            <button type="button" disabled={busy} onClick={onDelete}>
              저장 키 삭제
            </button>
          )}
        </div>
        <p>
          {status?.configured
            ? `키 설정됨 · ${status.source}`
            : "키 없음"}
        </p>
      </div>
    </section>
  );
}
