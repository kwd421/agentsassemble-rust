import { useEffect, useRef, useState } from "react";
import { Play, Plus, X } from "lucide-react";
import {
  deleteProviderCredential,
  fetchProviderCredentialStatus,
  setProviderCredential,
  type FrontendLiveAgentCreateRequest,
  type ProviderCredentialStatus,
} from "../../api";
import type { NativeCliProviderAvailability, ProviderControl } from "../../roomSocketClient";
import {
  displayProviderControls,
  effectiveProviderControlOptions,
  initializeProviderSettings,
  reconcileProviderSettings,
} from "../../lib/providerControlSettings";
import {
  PROVIDER_GROUPS,
  projectProvidersByCatalogGroup,
  providerCatalogGroup,
  providerGroupLabel,
  type ProviderCatalogGroup,
} from "../../lib/providerCatalogGroups";
import ProviderLogo from "./ProviderLogo";
import ProviderControlSelect from "./ProviderControlSelect";
import ProviderControlToggle from "./ProviderControlToggle";
import AgentPersonaPicker from "./AgentPersonaPicker";
import ProviderCredentialField from "./ProviderCredentialField";
import WorkspacePickerField from "./WorkspacePickerField";
import { resolveProviderPresentation } from "./providerBranding";
import {
  defaultAgentDisplayName,
  deriveAgentCreateStatus,
} from "./agentCreateModel";

type AgentCreateModalProps = {
  open: boolean;
  meetingId: string;
  roomLabel: string;
  providers: NativeCliProviderAvailability[];
  catalogRevision?: string;
  onClose: () => void;
  onCreate: (request: FrontendLiveAgentCreateRequest) => Promise<void>;
  onCreated?: () => void;
};

export default function AgentCreateModal({
  open,
  meetingId,
  roomLabel,
  providers,
  catalogRevision = "",
  onClose,
  onCreate,
  onCreated,
}: AgentCreateModalProps) {
  const [providerGroup, setProviderGroup] = useState<ProviderCatalogGroup | "">(
    "harness"
  );
  const [providerId, setProviderId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [displayNameEdited, setDisplayNameEdited] = useState(false);
  const [workspacePath, setWorkspacePath] = useState("");
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [startNow, setStartNow] = useState(false);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [providerApiKey, setProviderApiKey] = useState("");
  const [personaCardId, setPersonaCardId] = useState("");
  const [credentialStatus, setCredentialStatus] = useState<ProviderCredentialStatus | null>(null);
  const [credentialBusy, setCredentialBusy] = useState(false);
  const wasOpen = useRef(false);
  const groupedProviders = projectProvidersByCatalogGroup(providers);
  const visibleProviders = providerGroup ? groupedProviders[providerGroup] : [];
  const selectedProvider = visibleProviders.find((provider) => provider.id === providerId);
  const workspaceRequired = Boolean(
    selectedProvider && (
      selectedProvider.workspace_required ||
      settings.permission_mode === "workspace_write"
    )
  );
  const selectedProviderMissing = Boolean(providerId && providers.length && !selectedProvider);
  const invalidControl = !selectedProvider
    ? undefined
    : selectedProvider.controls.find((control) =>
        !effectiveProviderControlOptions(selectedProvider, control, settings).some(
          (option) => option.value === (settings[control.key] ?? "")
        )
      );
  const canCreate = Boolean(
    meetingId &&
      selectedProvider &&
      catalogRevision &&
      selectedProvider.startable &&
      !invalidControl &&
      displayName.trim() &&
      (!workspaceRequired || workspacePath.trim())
  );
  const statusMessage = deriveAgentCreateStatus({
    status,
    workspacePath,
    selectedProvider,
    selectedProviderMissing,
    hasProviders: providers.length > 0,
    invalidControl,
    workspaceRequired,
  });

  useEffect(() => {
    if (!open) {
      wasOpen.current = false;
      setPersonaCardId("");
      return;
    }
    if (!wasOpen.current) {
      setWorkspacePath("");
    }
    if (!providers.length) return;
    const current = visibleProviders.find((provider) => provider.id === providerId);
    if (!wasOpen.current) {
      if (current) applyProvider(current);
      setStatus("");
      wasOpen.current = true;
      return;
    }
    if (current) {
      setSettings((previous) => reconcileProviderSettings(current, previous));
    }
    wasOpen.current = true;
  }, [open, providers, providerGroup, providerId]);

  useEffect(() => {
    if (!open || !selectedProvider || displayNameEdited) return;
    setDisplayName(defaultAgentDisplayName(selectedProvider, settings));
  }, [displayNameEdited, open, selectedProvider, settings]);

  useEffect(() => {
    if (
      !open ||
      !selectedProvider ||
      !selectedProvider.credential_available
    ) {
      setProviderApiKey("");
      setCredentialStatus(null);
      return;
    }
    setCredentialStatus(null);
    fetchProviderCredentialStatus(selectedProvider.id)
      .then(setCredentialStatus)
      .catch((error) => setStatus(error instanceof Error ? error.message : "키 상태 확인 실패"));
  }, [open, selectedProvider?.id, selectedProvider?.credential_available]);

  function applyProvider(provider: NativeCliProviderAvailability) {
    const initialSettings = initializeProviderSettings(provider);
    setProviderGroup(providerCatalogGroup(provider));
    setProviderId(provider.id);
    setDisplayName(defaultAgentDisplayName(provider, initialSettings));
    setDisplayNameEdited(false);
    setSettings(initialSettings);
    setPersonaCardId("");
    setStartNow(provider.startable);
  }

  function chooseProviderGroup(group: ProviderCatalogGroup) {
    setProviderGroup(group);
    setProviderId("");
    setDisplayName("");
    setDisplayNameEdited(false);
    setSettings({});
    setPersonaCardId("");
    setStartNow(false);
    setStatus("");
  }

  function updateProviderControl(key: string, value: string) {
    if (!selectedProvider) return;
    const next = reconcileProviderSettings(
      selectedProvider,
      {
        ...settings,
        [key]: value,
      },
      key
    );
    setSettings(next);
    if (key === "model" && !displayNameEdited) {
      setDisplayName(defaultAgentDisplayName(selectedProvider, next));
    }
  }

  async function handleCreate() {
    if (!canCreate || !selectedProvider) {
      setStatus(
        invalidControl
          ? `${invalidControl.label} 선택값을 확인하세요`
          : selectedProvider?.discovery_error || "실행 가능한 provider와 폴더를 확인하세요"
      );
      return;
    }
    setBusy(true);
    setStatus(startNow ? "에이전트 시작 중..." : "에이전트 추가 중...");
    try {
      await onCreate({
        meetingId,
        providerId: selectedProvider.id,
        catalogRevision,
        displayName,
        workspacePath,
        modelId: settings.model || "",
        reasoningEffort: settings.reasoning_effort || "",
        serviceTier: settings.service_tier || "",
        variant: settings.variant || "",
        permissionMode: settings.permission_mode || "meeting_read_only",
        maxOutputTokens: Number(settings.max_output_tokens || 0),
        personaCardId,
        startNow,
      });
      onCreated?.();
      onClose();
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "에이전트 추가 실패");
    } finally {
      setBusy(false);
    }
  }

  async function saveProviderApiKey() {
    if (!selectedProvider || !providerApiKey.trim() || credentialBusy) return;
    setCredentialBusy(true);
    try {
      const credentialStatus = await setProviderCredential(
        selectedProvider.id,
        providerApiKey
      );
      setCredentialStatus(credentialStatus);
      setProviderApiKey("");
      setStatus(`${selectedProvider.display_name} 키가 서버의 보안 저장소에 저장됐습니다`);
    } catch (error) {
      setStatus(
        error instanceof Error
          ? error.message
          : `${selectedProvider.display_name} 키 저장 실패`
      );
    } finally {
      setCredentialBusy(false);
    }
  }

  async function deleteProviderApiKey() {
    if (!selectedProvider || credentialBusy) return;
    setCredentialBusy(true);
    setStatus("");
    try {
      setCredentialStatus(await deleteProviderCredential(selectedProvider.id));
      setProviderApiKey("");
      setStatus(`${selectedProvider.display_name} 저장 키를 삭제했습니다`);
    } catch (error) {
      setStatus(
        error instanceof Error
          ? error.message
          : `${selectedProvider.display_name} 키 삭제 실패`
      );
    } finally {
      setCredentialBusy(false);
    }
  }

  function renderProviderChoice(provider: NativeCliProviderAvailability) {
    const presentation = resolveProviderPresentation({
      providerId: provider.id,
      providerKind: provider.provider_kind,
      providerDisplayName: provider.display_name,
    });
    return (
      <button
        key={provider.id}
        type="button"
        role="listitem"
        aria-label={presentation.providerName}
        title={presentation.providerName}
        data-active={provider.id === selectedProvider?.id}
        disabled={!provider.available}
        onClick={() => {
          applyProvider(provider);
          setStatus("");
        }}
      >
        <ProviderLogo
          providerId={provider.id}
          providerKind={provider.provider_kind}
          size={22}
        />
        <span className="min-w-0 truncate">{presentation.providerName}</span>
      </button>
    );
  }

  if (!open) return null;

  return (
    <div className="dc-modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="dc-agent-create-modal"
        role="dialog"
        aria-modal="true"
        aria-label="에이전트 추가"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="dc-agent-create-head">
          <div>
            <p className="dc-agent-create-kicker preserve-words">{roomLabel}</p>
            <h2>에이전트 추가</h2>
          </div>
          <button type="button" onClick={onClose} aria-label="닫기">
            <X size={18} />
          </button>
        </header>

        <div className="dc-agent-create-body">
          <section className="dc-agent-section">
            <p className="dc-agent-section-title">종류</p>
            <div className="dc-agent-provider-grid" role="list" aria-label="에이전트 종류">
              {PROVIDER_GROUPS.map(({ id, label, Icon }) => (
                <button
                  key={id}
                  type="button"
                  role="listitem"
                  aria-label={label}
                  data-active={providerGroup === id}
                  disabled={groupedProviders[id].length === 0}
                  onClick={() => chooseProviderGroup(id)}
                >
                  <Icon size={22} aria-hidden="true" />
                  <span>{label}</span>
                </button>
              ))}
            </div>
          </section>

          {providerGroup && (
            <section className="dc-agent-section">
              <p className="dc-agent-section-title">
                {providerGroupLabel(providerGroup)} Providers
              </p>
              <div
                className="dc-agent-provider-grid"
                role="list"
                aria-label={
                  providerGroup === "api"
                    ? "API 프로바이더"
                    : `${providerGroupLabel(providerGroup)} Providers`
                }
              >
                {visibleProviders.map(renderProviderChoice)}
              </div>
            </section>
          )}

          {selectedProvider && (
            <section className="dc-agent-section">
              <p className="dc-agent-section-title">기본 정보</p>
              <div className="dc-agent-field-grid">
              <label className="dc-agent-field">
                <span>표시 이름</span>
                <input
                  value={displayName}
                  placeholder="방에 표시될 이름"
                  onChange={(event) => {
                    setDisplayName(event.currentTarget.value);
                    setDisplayNameEdited(true);
                  }}
                />
              </label>
              {workspaceRequired && (
                <WorkspacePickerField
                  value={workspacePath}
                  description={
                    settings.permission_mode === "workspace_write"
                      ? "API 모델이 이 폴더의 텍스트를 읽을 수 있습니다. 파일 변경과 명령 실행은 매번 승인을 요청합니다."
                      : ""
                  }
                  onChange={setWorkspacePath}
                  onError={setStatus}
                />
              )}
              </div>
            </section>
          )}

          {selectedProvider && selectedProvider.controls.length > 0 && (
            <section className="dc-agent-section">
              <p className="dc-agent-section-title">모델 · 실행 설정</p>
              <div className="dc-agent-field-grid dc-agent-field-grid--dual">
                {displayProviderControls(selectedProvider).map((control) => {
                  const providerSupportsControl = selectedProvider.controls.some(
                    (candidate) => candidate.key === control.key
                  );
                  const options = providerSupportsControl
                    ? effectiveProviderControlOptions(
                        selectedProvider,
                        control,
                        settings
                      )
                    : control.options;
                  return (
                    <ProviderControlField
                      key={`${selectedProvider.id}:${control.key}`}
                      control={control}
                      options={options}
                      value={
                        providerSupportsControl
                          ? settings[control.key] ?? control.default_value
                          : control.default_value
                      }
                      disabled={!providerSupportsControl}
                      onChange={(value) => updateProviderControl(control.key, value)}
                    />
                  );
                })}
              </div>
            </section>
          )}

          {selectedProvider?.credential_available && (
            <ProviderCredentialField
              provider={selectedProvider}
              status={credentialStatus}
              value={providerApiKey}
              busy={credentialBusy}
              onValueChange={setProviderApiKey}
              onSave={() => void saveProviderApiKey()}
              onDelete={() => void deleteProviderApiKey()}
            />
          )}

          {selectedProvider &&
            ["api", "local"].includes(providerCatalogGroup(selectedProvider)) && (
              <section className="dc-agent-section">
                <AgentPersonaPicker value={personaCardId} onChange={setPersonaCardId} />
              </section>
            )}

          {statusMessage && (
            <p className="dc-agent-create-status preserve-words">{statusMessage}</p>
          )}
        </div>

        <footer className="dc-agent-create-footer">
          <button
            type="button"
            className="dc-agent-launch-toggle"
            role="switch"
            aria-checked={startNow}
            aria-label="추가하자마자 실행"
            data-on={startNow}
            disabled={!selectedProvider?.startable}
            onClick={() => setStartNow((value) => !value)}
          >
            <span className="dc-agent-launch-switch" aria-hidden="true">
              <i />
            </span>
            <span className="dc-agent-launch-text">
              <strong>{startNow ? "추가하고 바로 실행" : "목록에만 추가"}</strong>
              <em>
                {startNow
                  ? "추가와 동시에 세션이 켜집니다."
                  : "카드에서 언제든 켤 수 있어요."}
              </em>
            </span>
          </button>
          <div className="dc-agent-footer-actions">
            <button type="button" className="dc-agent-create-secondary" onClick={onClose}>
              취소
            </button>
            <button
              type="button"
              className="dc-agent-create-primary"
              disabled={!canCreate || busy}
              onClick={() => void handleCreate()}
            >
              {startNow ? <Play size={16} /> : <Plus size={16} />}
              {busy ? "처리 중..." : startNow ? "추가하고 실행" : "추가"}
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}

function ProviderControlField({
  control,
  options,
  value,
  onChange,
  disabled = false,
}: {
  control: ProviderControl;
  options: ProviderControl["options"];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  return (
    <label className="dc-agent-field">
      <span>{control.label}</span>
      {control.key === "service_tier" && options.length <= 2 ? (
        <ProviderControlToggle
          label={control.label}
          options={options}
          value={value}
          disabled={disabled}
          onChange={onChange}
        />
      ) : (
        <ProviderControlSelect
          label={control.label}
          options={options}
          value={value}
          disabled={disabled}
          onChange={onChange}
        />
      )}
    </label>
  );
}
