import { reasoningEffortLabel } from "../../lib/agentLabels";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowLeft, Play, Plus, X } from "../../ui/icons";
import {
  deleteProviderCredential,
  fetchProviderCredentialStatus,
  setProviderCredential,
  type FrontendLiveAgentCreateRequest,
  type RoomAgentSession,
  type RoomMember,
  type ProviderCredentialStatus,
} from "../../api";
import { RoomSocketSayError, type NativeCliProviderAvailability, type ProviderControl } from "../../roomSocketClient";
import {
  displayProviderControls,
  requiresOpenCodeApproval,
  OPENCODE_FREE_PERMISSION_HINT,
  effectiveProviderControlOptions,
  harnessIgnoresControl,
  initializeProviderSettings,
  reconcileProviderSettings,
} from "../../lib/providerControlSettings";
import {
  providerCatalogGroup,
  providerGroupLabel,
} from "../../lib/providerCatalogGroups";
import ProviderPicker from "./ProviderPicker";
import ProviderModelRefresh from "./ProviderModelRefresh";
import type { ProviderCatalog } from "../../types/generated/ProviderCatalog";
import { fetchLocalProviderCatalog, refreshLocalProviderCatalog } from "../../api/providerOperations";
import { isDesktopWebview } from "../../lib/desktopBridge";
import ProviderLogin from "./ProviderLogin";
import ProviderSetupActions from "./ProviderSetupActions";
import ProviderControlSelect from "./ProviderControlSelect";
import ProviderControlToggle from "./ProviderControlToggle";
import AgentPersonaPicker from "./AgentPersonaPicker";
import ProviderCredentialField from "./ProviderCredentialField";
import WorkspacePickerField from "./WorkspacePickerField";
import { userFacingError } from "../../lib/apiErrors";
import {
  defaultAgentDisplayName,
  deriveAgentCreateStatus,
  providerUnavailableText,
} from "./agentCreateModel";

type AgentCreateModalProps = {
  locationChoice?: ReactNode;
  open: boolean;
  meetingId: string;
  roomLabel: string;
  roomLabelMuted?: boolean;
  providers: NativeCliProviderAvailability[];
  existingSessions?: RoomAgentSession[];
  participants?: RoomMember[];
  catalogRevision?: string;
  localProviderActions?: boolean;
  openCodeFreePermissionHint?: string;
  /** The room's own host: persona library reads stay on that host. */
  serverId?: string;
  initialSelection?: { providerId: string; displayName: string };
  onCatalogChange?: (catalog: ProviderCatalog) => void;
  onClose: () => void;
  onCreate: (request: FrontendLiveAgentCreateRequest) => Promise<void>;
  onCreated?: () => void;
};

export default function AgentCreateModal({
  open,
  locationChoice,
  meetingId,
  roomLabel,
  roomLabelMuted = false,
  providers,
  existingSessions = [],
  participants = [],
  catalogRevision = "",
  localProviderActions = true,
  openCodeFreePermissionHint = OPENCODE_FREE_PERMISSION_HINT,
  serverId,
  initialSelection,
  onCatalogChange,
  onClose,
  onCreate,
  onCreated,
}: AgentCreateModalProps) {
  const [providerId, setProviderId] = useState("");
  const [existingSessionId, setExistingSessionId] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [displayNameEdited, setDisplayNameEdited] = useState(false);
  const [workspacePath, setWorkspacePath] = useState("");
  const [customEndpoint, setCustomEndpoint] = useState("");
  const [customModel, setCustomModel] = useState("");
  const [agentPlatformProject, setAgentPlatformProject] = useState("");
  const [agentPlatformLocation, setAgentPlatformLocation] = useState("global");
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [startNow, setStartNow] = useState(false);
  const [executionHarness, setExecutionHarness] = useState<"builtin" | "opencode">("builtin");
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [updatingProviders, setUpdatingProviders] = useState<Set<string>>(() => new Set());
  const [providerApiKey, setProviderApiKey] = useState("");
  const [personaCardId, setPersonaCardId] = useState("");
  const [credentialStatus, setCredentialStatus] = useState<ProviderCredentialStatus | null>(null);
  const [credentialBusy, setCredentialBusy] = useState(false);
  const createRetry = useRef<{ key: string; retry: NonNullable<RoomSocketSayError["retry"]> } | null>(null);
  const wasOpen = useRef(false);
  const pendingInitialSettings = useRef("");
  const selectedProvider = providers.find((provider) => provider.id === providerId);
  // Two steps: the AI list, then that AI's settings sliding in from the right.
  const [picking, setPicking] = useState(true);
  const [slide, setSlide] = useState<"forward" | "back" | null>(null);
  const showPicker = picking || !selectedProvider;
  const agentPlatformProvider = selectedProvider?.provider_kind === "agent_platform_api";
  const agentPlatformSettings =
    agentPlatformProvider && agentPlatformProject.trim() && agentPlatformLocation.trim()
      ? `projects/${agentPlatformProject.trim()}/locations/${agentPlatformLocation.trim()}`
      : "";
  const callerEndpoint = agentPlatformProvider ? agentPlatformSettings : customEndpoint.trim();
  const callerEndpointRequired = Boolean(
    selectedProvider && (selectedProvider.custom_endpoint || agentPlatformProvider)
  );
  const providerGroup = selectedProvider ? providerCatalogGroup(selectedProvider, settings.model) : "";
  const openCodeHarnessAvailable = Boolean(
    !existingSessionId &&
      selectedProvider &&
      ["gemini_api", "deepseek_api"].includes(selectedProvider.provider_kind) &&
      providers.some((provider) => provider.id === "opencode")
  );
  const eligibleStoredSessions = existingSessions.filter((session) =>
    session.room_id === meetingId && !session.external_owned && session.process_ownership === "server" &&
    ["stopped", "error"].includes(session.runtime_status) && !session.enabled &&
    !session.recovery_required && !session.provider_session_active && !session.active_turn_id &&
    !participants.some((participant) => participant.room_id === session.room_id &&
      participant.participant_id === session.participant_id && participant.status === "exported")
  );
  const reusableSessions = eligibleStoredSessions.filter((session) =>
    session.provider_kind === selectedProvider?.provider_kind
  );
  const existingSession = reusableSessions.find((session) => session.session_id === existingSessionId);
  const workspaceRequired = Boolean(
    selectedProvider && (
      selectedProvider.workspace_required ||
      ["workspace_write", "full_access"].includes(settings.permission_mode)
    )
  );
  const selectedProviderMissing = Boolean((providerId || initialSelection?.providerId) &&
    !providers.some((provider) => provider.id === (providerId || initialSelection?.providerId)));
  const invalidControl = existingSessionId || !selectedProvider
    ? undefined
    : selectedProvider.controls.find((control) =>
        !harnessIgnoresControl(executionHarness, control.key) &&
        !effectiveProviderControlOptions(selectedProvider, control, settings).some(
          (option) => option.value === (settings[control.key] ?? "")
        )
      );
  const canCreate = Boolean(
    meetingId && selectedProvider && !updatingProviders.has(selectedProvider.id) && (
      existingSessionId
        ? existingSession && (!startNow || selectedProvider.startable)
        : catalogRevision && selectedProvider.startable && !invalidControl && displayName.trim() &&
          (!callerEndpointRequired || callerEndpoint) &&
          (!selectedProvider.custom_model || customModel.trim()) &&
          (!workspaceRequired || workspacePath.trim())
    )
  );
  const statusMessage = deriveAgentCreateStatus({
    status: status || (selectedProvider?.credential_available && credentialStatus?.configured === false
      ? "API 키를 연결하면 모델 목록을 확인할 수 있어요." : ""),
    workspacePath,
    selectedProvider,
    selectedProviderMissing,
    hasProviders: providers.length > 0,
    invalidControl,
    workspaceRequired: !existingSessionId && workspaceRequired,
  });

  useEffect(() => {
    if (!open) {
      createRetry.current = null;
      wasOpen.current = false;
      setExistingSessionId("");
      setCustomEndpoint("");
      setCustomModel("");
      setAgentPlatformProject("");
      setAgentPlatformLocation("global");
      setPersonaCardId("");
      setExecutionHarness("builtin");
      return;
    }
    if (!wasOpen.current) {
      setWorkspacePath("");
    }
    if (!providers.length) return;
    const current = providers.find((provider) => provider.id === providerId);
    if (!wasOpen.current) {
      const initial = initialSelection && providers.find((item) => item.id === initialSelection.providerId);
      if (initial) {
        applyProvider(initial);
        setDisplayName(initialSelection!.displayName);
        setDisplayNameEdited(true);
      } else if (current) applyProvider(current);
      setPicking(!initial);
      setSlide(null);
      setStatus("");
      wasOpen.current = true;
      return;
    }
    if (current && !existingSessionId) {
      if (pendingInitialSettings.current === current.id && current.discovery_status === "ready") {
        pendingInitialSettings.current = "";
        setSettings(initializeProviderSettings(current));
      } else {
        setSettings((previous) => reconcileProviderSettings(current, previous));
      }
    }
    wasOpen.current = true;
  }, [open, providers, providerId, existingSessionId]);

  useEffect(() => {
    if (!open || !selectedProvider || existingSessionId || displayNameEdited) return;
    setDisplayName(defaultAgentDisplayName(selectedProvider, settings));
  }, [displayNameEdited, existingSessionId, open, selectedProvider, settings]);

  useEffect(() => {
    if (
      !open ||
      !localProviderActions ||
      !selectedProvider ||
      !selectedProvider.credential_available
    ) {
      setProviderApiKey("");
      setCredentialStatus(null);
      return;
    }
    setCredentialStatus(null);
    let current = true;
    fetchProviderCredentialStatus(selectedProvider.id)
      .then((result) => { if (current) setCredentialStatus(result); })
      .catch((error) => { if (current) setStatus(userFacingError(error, "키 상태 확인 실패")); });
    return () => { current = false; };
  }, [open, localProviderActions, selectedProvider?.id, selectedProvider?.credential_available]);

  function applyProvider(provider: NativeCliProviderAvailability) {
    setExistingSessionId("");
    const initialSettings = initializeProviderSettings(provider);
    pendingInitialSettings.current = provider.controls.length ? "" : provider.id;
    setProviderId(provider.id);
    if (!wasOpen.current || !displayNameEdited) {
      setDisplayName(defaultAgentDisplayName(provider, initialSettings));
      setDisplayNameEdited(false);
    }
    setSettings(initialSettings);
    setCustomEndpoint("");
    setCustomModel("");
    setAgentPlatformProject("");
    setAgentPlatformLocation(
      provider.provider_kind === "agent_platform_api" ? "global" : ""
    );
    setPersonaCardId("");
    setExecutionHarness("builtin");
    setStartNow(provider.startable || provider.discovery_status === "loading");
  }

  function applyExistingSession(sessionId: string) {
    const session = reusableSessions.find((item) => item.session_id === sessionId);
    if (!session) {
      if (selectedProvider) applyProvider(selectedProvider);
      return;
    }
    setExistingSessionId(sessionId);
    setDisplayName(session.display_name);
    setDisplayNameEdited(true);
    setSettings({
      model: session.model,
      reasoning_effort: session.reasoning_effort,
      service_tier: session.service_tier,
      variant: session.variant,
      permission_mode: session.permission_mode,
      max_output_tokens: String(session.max_output_tokens),
    });
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
    if (providerCatalogGroup(selectedProvider, next.model) === "harness") setPersonaCardId("");
    if (key === "model" && !displayNameEdited) {
      setDisplayName(defaultAgentDisplayName(selectedProvider, next));
    }
  }

  async function handleCreate() {
    if (!canCreate || !selectedProvider) {
      setStatus(
        invalidControl
          ? `${invalidControl.label} 선택값을 확인하세요`
          : selectedProvider &&
            ["command_missing", "bridge_runtime_missing"].includes(selectedProvider.discovery_error_code || "")
            ? providerUnavailableText(selectedProvider)
          : selectedProvider?.discovery_error || "실행할 AI와 작업 폴더를 확인해 주세요."
      );
      return;
    }
    setBusy(true);
    setStatus(startNow ? "에이전트 시작 중..." : "에이전트 추가 중...");
    const request: FrontendLiveAgentCreateRequest = {
      meetingId,
      sessionId: existingSessionId || undefined,
      providerId: selectedProvider.id,
      catalogRevision,
      displayName,
      workspacePath,
      providerEndpoint: callerEndpointRequired ? callerEndpoint : "",
      modelId: selectedProvider.custom_model
        ? customModel.trim()
        : settings.model || "",
      reasoningEffort: settings.reasoning_effort || "",
      serviceTier: settings.service_tier || "",
      variant: settings.variant || "",
      executionHarness,
      permissionMode: settings.permission_mode || "meeting_read_only",
      ...(settings.permission_mode === "full_access" ? { fullAccessAcknowledged: true as const } : {}),
      maxOutputTokens: Number(settings.max_output_tokens || 0),
      personaCardId,
      startNow,
    };
    // Discovery freshness is not a new user intent; retry keeps its original payload.
    const key = JSON.stringify({ ...request, catalogRevision: undefined });
    if (createRetry.current?.key !== key) createRetry.current = null;
    try {
      if (createRetry.current) await createRetry.current.retry();
      else await onCreate(request);
      createRetry.current = null;
      onCreated?.();
      onClose();
    } catch (error) {
      if (error instanceof RoomSocketSayError && error.retry) {
        createRetry.current = { key, retry: error.retry };
      }
      setStatus(userFacingError(error, "에이전트 추가 실패"));
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
      // The credential status area already reflects the saved key; no success box.
      setStatus("");
      if (localProviderActions && isDesktopWebview()) {
        const catalog = await refreshLocalProviderCatalog(selectedProvider.id);
        onCatalogChange?.(catalog);
      }
    } catch (error) {
      setStatus(
        userFacingError(error, `${selectedProvider.display_name} 키를 저장하지 못했어요.`)
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
      // The credential status area already reflects the removal; no success box.
      setStatus("");
      if (localProviderActions && isDesktopWebview()) {
        const catalog = await refreshLocalProviderCatalog(selectedProvider.id);
        onCatalogChange?.(catalog);
      }
    } catch (error) {
      setStatus(
        userFacingError(error, `${selectedProvider.display_name} 키를 삭제하지 못했어요.`)
      );
    } finally {
      setCredentialBusy(false);
    }
  }

  async function readUpdatedCatalog() {
    if (!onCatalogChange) return;
    try { onCatalogChange(await fetchLocalProviderCatalog()); }
    catch (error) { setStatus(userFacingError(error, "AI 상태를 확인하지 못했어요.")); }
  }

  if (!open) return null;

  return (
    <div className="dc-modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section
        className="dc-agent-create-modal"
        data-provider-selected={!showPicker}
        role="dialog"
        aria-modal="true"
        aria-label="에이전트 추가"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="dc-agent-create-head">
          {!showPicker && (
            <button type="button" className="dc-agent-create-back" aria-label="AI 다시 고르기" title="AI 다시 고르기"
              onClick={() => { setPicking(true); setSlide("back"); }}>
              <ArrowLeft size={18} />
            </button>
          )}
          <div className="dc-agent-create-title">
            <p className={`dc-agent-create-kicker preserve-words${roomLabelMuted ? " dc-agent-host-address" : ""}`}>{roomLabel}</p>
            <h2>에이전트 추가</h2>
          </div>
          <div className="dc-agent-head-actions">
            <ProviderModelRefresh
              onCatalogChange={onCatalogChange}
              localAvailable={localProviderActions}
              title="AI"
              compact
              providerId={existingSessionId ? "" : selectedProvider?.id || ""}
              automaticAllowed={Boolean(selectedProvider &&
                selectedProvider.discovery_error_code !== "authentication_required" &&
                (!selectedProvider.credential_available || credentialStatus?.configured))}
            />
            <button type="button" onClick={onClose} aria-label="닫기">
              <X size={18} />
            </button>
          </div>
        </header>

        {showPicker ? (
        <div className="dc-agent-create-body" key="pick" data-slide={slide === "back" ? "back" : undefined}>
          {locationChoice}
          {!selectedProvider && statusMessage && <p className="dc-agent-create-note">{statusMessage}</p>}
          <ProviderPicker providers={providers} selectedId={providerId} view="list"
            onSelect={(provider) => {
              // Picking the same AI again keeps what was already set for it.
              if (provider.id !== providerId) applyProvider(provider);
              setStatus("");
              setPicking(false);
              setSlide("forward");
            }} />
        </div>
        ) : (
        <div className="dc-agent-create-body" key="settings" data-slide={slide === "forward" ? "forward" : undefined}>
          <ProviderPicker providers={providers} selectedId={providerId} view="card"
            onSelect={(provider) => { applyProvider(provider); setStatus(""); }} />

          {localProviderActions && !existingSessionId && selectedProvider?.login_supported &&
            selectedProvider.discovery_error_code === "authentication_required" &&
            <ProviderLogin key={selectedProvider.id} providerId={selectedProvider.id}
              displayName={selectedProvider.display_name} automatic onAuthenticated={() => void readUpdatedCatalog()} />}
          {localProviderActions && selectedProvider && <ProviderSetupActions key={`setup-${selectedProvider.id}`} providerId={selectedProvider.id}
            provider={selectedProvider} localAvailable={localProviderActions} onUpdated={() => void readUpdatedCatalog()}
            onUpdating={(updating) => setUpdatingProviders((current) => {
              const next = new Set(current);
              if (updating) next.add(selectedProvider.id);
              else next.delete(selectedProvider.id);
              return next;
            })} />}
          {selectedProvider?.custom_endpoint && !existingSessionId && (
            <section className="dc-agent-section">
              <p className="dc-agent-section-title">API 연결</p>
              <div className="dc-agent-field-grid dc-agent-field-grid--dual">
                <label className="dc-agent-field">
                  <span>API 주소</span>
                  <input
                    type="url"
                    value={customEndpoint}
                    placeholder="https://example.com/v1 또는 …/chat/completions"
                    onChange={(event) => setCustomEndpoint(event.currentTarget.value)}
                  />
                </label>
                <label className="dc-agent-field">
                  <span>모델 ID</span>
                  <input
                    value={customModel}
                    placeholder="사용할 AI의 정확한 모델 ID"
                    onChange={(event) => {
                      const value = event.currentTarget.value;
                      setCustomModel(value);
                      if (!displayNameEdited) {
                        setDisplayName(value.trim() ? `Custom ${value.trim()}` : "Custom API");
                      }
                    }}
                  />
                </label>
              </div>
              <p className="preserve-words">
                Base URL과 /chat/completions 전체 주소를 모두 받을 수 있어요.
              </p>
            </section>
          )}

          {localProviderActions && selectedProvider?.credential_available && (
            <ProviderCredentialField
              provider={selectedProvider}
              status={credentialStatus}
              value={providerApiKey}
              busy={credentialBusy}
              onValueChange={setProviderApiKey}
              onSave={() => void saveProviderApiKey()}
              onDelete={() => void deleteProviderApiKey()}
            >
              {agentPlatformProvider && !existingSessionId && (
                <div className="dc-agent-field-grid dc-agent-field-grid--dual">
                  <label className="dc-agent-field">
                    <span>프로젝트 ID</span>
                    <input
                      value={agentPlatformProject}
                      placeholder="Google Cloud 프로젝트 ID"
                      onChange={(event) => setAgentPlatformProject(event.currentTarget.value)}
                    />
                  </label>
                  <label className="dc-agent-field">
                    <span>지역</span>
                    <input
                      value={agentPlatformLocation}
                      placeholder="global"
                      onChange={(event) => setAgentPlatformLocation(event.currentTarget.value)}
                    />
                  </label>
                </div>
              )}
            </ProviderCredentialField>
          )}

          {selectedProvider && (selectedProvider.controls.length > 0 || localProviderActions) && (
            <section className="dc-agent-section">
              <p className="dc-agent-section-title">모델 · 실행 설정</p>

              <div className="dc-agent-field-grid dc-agent-field-grid--dual">
                {openCodeHarnessAvailable && (
                  <div className="dc-agent-field">
                    <span>하네스</span>
                    <ProviderControlSelect
                      label="하네스"
                      options={[
                        { value: "builtin", label: "기본" },
                        { value: "opencode", label: "OpenCode" },
                      ]}
                      value={executionHarness}
                      onChange={(value) =>
                        setExecutionHarness(value === "opencode" ? "opencode" : "builtin")
                      }
                    />
                    {executionHarness === "opencode" && (
                      <p className="dc-agent-hint preserve-words">
                        OpenCode가 이 API 키로 실행돼요. 파일·명령 도구를 쓸 수 있어요.
                      </p>
                    )}
                  </div>
                )}
                {requiresOpenCodeApproval(selectedProvider, settings) && (
                  <p className="preserve-words" style={{ gridColumn: "1 / -1", color: "var(--color-text-muted)", fontSize: 12 }}>{openCodeFreePermissionHint}</p>
                )}
                {selectedProvider.controls.length > 0 && displayProviderControls(selectedProvider)
                  .filter((control) => !harnessIgnoresControl(executionHarness, control.key))
                  .map((control) => {
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
                      options={existingSessionId
                        ? [{ value: settings[control.key] ?? "", label: settings[control.key] || "미설정" }]
                        : control.key === "model" && new Set(options.map((option) =>
                            providerCatalogGroup(selectedProvider, option.value))).size > 1
                          ? options.map((option) => ({ ...option,
                              label: `${option.label} · ${providerGroupLabel(providerCatalogGroup(selectedProvider, option.value))}` }))
                          : options}
                      value={
                        existingSessionId ? settings[control.key] ?? "" : providerSupportsControl
                          ? settings[control.key] ?? control.default_value
                          : control.default_value
                      }
                      disabled={Boolean(existingSessionId) || !providerSupportsControl}
                      onChange={(value) => updateProviderControl(control.key, value)}
                    />
                  );
                })}
              </div>
            </section>
          )}

          {selectedProvider && (
            <section className="dc-agent-section">
              <p className="dc-agent-section-title">기본 정보</p>
              <div className="dc-agent-field-grid">
              {reusableSessions.length > 0 && (
                <label className="dc-agent-field">
                  <span>기존 세션</span>
                  <ProviderControlSelect
                    label="기존 세션"
                    options={[
                      { value: "", label: "새 세션 만들기" },
                      ...reusableSessions.map((session) => ({
                        value: session.session_id,
                        label: `${session.display_name} · ${session.model || session.provider_kind}`,
                      })),
                    ]}
                    value={existingSessionId}
                    onChange={applyExistingSession}
                  />
                </label>
              )}
              <label className="dc-agent-field">
                <span>표시 이름</span>
                <input
                  disabled={Boolean(existingSessionId)}
                  value={displayName}
                  placeholder="방에 표시될 이름"
                  onChange={(event) => {
                    setDisplayName(event.currentTarget.value);
                    setDisplayNameEdited(true);
                  }}
                />
              </label>
              {!existingSessionId && workspaceRequired && (
                <WorkspacePickerField
                  value={workspacePath}
                  description={
                    settings.permission_mode === "workspace_write" && providerGroup !== "harness"
                      ? "명령 실행·파일 수정은 할 때마다 승인해요"
                      : ""
                  }
                  onChange={setWorkspacePath}
                  onError={setStatus}
                />
              )}
              </div>
            </section>
          )}

          {selectedProvider && !existingSessionId &&
            ["api", "local"].includes(providerGroup) && (
              <section className="dc-agent-section">
                <AgentPersonaPicker value={personaCardId} serverId={serverId} onChange={setPersonaCardId} />
              </section>
            )}

          {selectedProvider && statusMessage && (
            <p className="dc-agent-create-status preserve-words">{statusMessage}</p>
          )}
        </div>
        )}

        {!showPicker && <footer className="dc-agent-create-footer">
          <button
            type="button"
            className="dc-agent-launch-toggle"
            role="switch"
            aria-checked={startNow}
            aria-label="추가하자마자 실행"
            data-on={startNow}
            disabled={!selectedProvider?.startable && !startNow}
            onClick={() => setStartNow((value) => !value)}
          >
            <span className="dc-agent-launch-switch" aria-hidden="true">
              <i />
            </span>
            <span className="dc-agent-launch-text">
              <strong>바로 실행</strong>
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
        </footer>}
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
          options={control.key === "reasoning_effort"
            ? options.map((option) => ({ ...option, label: reasoningEffortLabel(option.value) })) : options}
          value={value}
          disabled={disabled}
          showSpeedIcon={control.key === "service_tier"}
          onChange={onChange}
        />
      )}
    </label>
  );
}
