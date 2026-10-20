import { useEffect, useMemo, useState } from "react";
import { CirclePause, LifeBuoy, Play, RotateCcw, Save, Square, Zap } from "../../ui/icons";
import type { RoomAgentSession } from "../../api";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import {
  canonicalProviderModelValue,
  displayProviderControls,
  requiresOpenCodeApproval,
  OPENCODE_FREE_PERMISSION_HINT,
  effectiveProviderControlOptions,
  reconcileProviderSettings,
} from "../../lib/providerControlSettings";
import AgentSessionPersonaSettings from "./AgentSessionPersonaSettings";
import AgentActivitySettings from "./AgentActivitySettings";
import ProviderModelRefresh from "./ProviderModelRefresh";
import { resolveProviderPresentation } from "./providerBranding";
import ProviderRuntimeSettingField from "./ProviderRuntimeSettingField";
import { userFacingError } from "../../lib/apiErrors";

export type AgentSessionControlAction = "start" | "pause" | "stop" | "resume" | "interrupt";

export function agentSessionStatusLabel(status?: string, recoveryRequired = false) {
  if (recoveryRequired) return "복구 필요";
  if (status === "busy") return "응답 중";
  if (status === "starting") return "시작 중";
  if (status === "idle") return "대기";
  if (status === "paused") return "일시정지";
  if (status === "stopping") return "중지 중";
  if (status === "stopped") return "중지됨";
  if (status === "available") return "시작 대기";
  if (status === "error") return "오류";
  if (status === "disconnected") return "연결 끊김";
  return status || "상태 미정";
}

export function agentSessionIsPresent(status?: string) {
  return ["starting", "idle", "busy", "paused", "stopping"].includes(status || "");
}

/** Whether the viewer may pause this session right now (owner of the detail-panel pause gate). */
export function agentSessionCanPause(session: RoomAgentSession) {
  return (
    !session.external_owned &&
    session.process_ownership === "server" &&
    session.runtime_status === "idle" &&
    !session.recovery_required
  );
}

/** Whether the viewer may resume this session right now (owner of the detail-panel resume gate). */
export function agentSessionCanResume(session: RoomAgentSession) {
  if (
    session.external_owned ||
    session.process_ownership !== "server" ||
    session.recovery_required
  ) {
    return false;
  }
  if (session.runtime_status === "paused") return true;
  return (
    Boolean(session.turn_count || session.last_seen_event_id) &&
    ["stopped", "error", "disconnected", "available"].includes(session.runtime_status || "")
  );
}

export function agentSessionPresenceStatus(status?: string, recoveryRequired = false) {
  if (recoveryRequired) return "error";
  if (status === "busy" || status === "starting" || status === "stopping") return "working";
  if (status === "idle") return "online";
  if (status === "paused" || status === "available") return "idle";
  if (status === "error") return "error";
  return "offline";
}

function actionCompletedLabel(action: AgentSessionControlAction) {
  if (action === "start") return "세션 시작 요청 완료";
  if (action === "pause") return "세션 일시정지 완료";
  if (action === "stop") return "세션 중지 요청 완료";
  if (action === "resume") return "세션 재개 요청 완료";
  return "현재 응답 중단 요청 완료";
}

/** One Korean line for the session, with raw provider text kept behind the disclosure. */
function sessionErrorInfo(session: RoomAgentSession): { summary: string; detail: string; status?: string } {
  const info = sessionErrorText(session);
  // The provider's HTTP status is the one detail worth showing beside the Korean line.
  const status = session.last_error_code === "provider_rate_limited"
    ? "429"
    : /\(HTTP (\d{3})\)/.exec(session.last_error || "")?.[1];
  return status && info.summary ? { ...info, status } : info;
}

function sessionErrorText(session: RoomAgentSession): { summary: string; detail: string } {
  if (session.last_error_code === "managed_bridge_exited") {
    return { summary: "에이전트 연결이 끊겼어요. 중지한 뒤 재개해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_output_truncated") {
    return { summary: "응답이 최대 응답 길이에서 잘렸어요. 실행 설정에서 최대 응답 길이를 늘리거나 더 짧게 요청해 주세요.", detail: "" };
  }
  if (session.last_error_code === "runtime_authority_uncertain") {
    return { summary: "앱이 갑자기 종료돼 이 세션이 실제로 끝났는지 확인하지 못했어요. 복구를 누르면 확인 후 정리해요.", detail: "" };
  }
  if (session.last_error_code === "provider_turn_recovery_required") {
    return { summary: "AI 응답이 완료됐는지 확인하지 못했어요. 복구를 눌러 실행 상태를 확인해 주세요.", detail: "" };
  }
  if (session.last_error_code === "quota_exhausted") {
    return { summary: "AI 서비스의 사용 한도나 잔액을 모두 썼어요.", detail: "" };
  }
  if (session.last_error_code === "provider_rate_limited") {
    return { summary: "AI 서비스에 요청이 너무 많아요. 잠시 후 다시 시도해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_api_busy") {
    return { summary: "AI 서비스가 지금 붐벼요. 잠시 후 다시 시도해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_api_unavailable") {
    return { summary: "AI 서비스에 연결하지 못했어요. 인터넷 연결을 확인하고 다시 시도해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_context_limit") {
    return { summary: "AI에게 보낼 대화 내용이 너무 커요. 다시 시도해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_credential_rejected") {
    return { summary: "AI 서비스가 API 키를 거절했어요. 키를 다시 확인해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_room_action_missing") {
    return { summary: "AI가 방에 답을 남기지 않고 끝냈어요. 다시 요청해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_room_read_missing") {
    return { summary: "AI가 방 내용을 읽지 않고 끝냈어요. 다시 요청해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_tool_round_limit") {
    return { summary: "AI가 도구를 너무 여러 번 써서 멈췄어요. 다시 요청해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_turn_timeout") {
    return { summary: "AI 응답 시간이 초과됐어요. 다시 시도해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_turn_cancelled") {
    return { summary: "AI 응답이 취소됐어요. 필요하면 다시 요청해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_model_mismatch") {
    return { summary: "선택한 모델 정보를 확인하지 못했어요. 실행 설정에서 모델을 다시 선택해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_runtime_exited") {
    return { summary: "에이전트 실행이 예기치 않게 끝났어요. 중지한 뒤 재개해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_runtime_restart_required") {
    return { summary: "에이전트를 다시 시작해야 해요. 중지한 뒤 재개해 주세요.", detail: "" };
  }
  if (session.last_error_code === "room_observation_unconfirmed") {
    return { summary: "대화 내용을 전달했는지 확인하지 못했어요. 다시 시도해 주세요.", detail: "" };
  }
  if (session.last_error_code === "room_portal_publication_missing") {
    return { summary: "AI가 방에 남길 답을 만들지 못했어요. 다시 요청해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_protocol_invalid") {
    return { summary: "AI 응답 형식을 이해하지 못했어요. 다시 시도해 주세요.", detail: "" };
  }
  if (session.last_error_code === "provider_turn_task_failed") {
    return { summary: "AI 실행이 끝나기 전에 연결이 끊겼어요. 다시 시도해 주세요.", detail: "" };
  }
  if (session.last_error_code === "interrupted") {
    return { summary: "운영자가 AI 응답을 중단했어요. 필요하면 다시 요청해 주세요.", detail: "" };
  }
  if (session.last_error_code === "bridge_disconnected") {
    return { summary: "외부 연결이 끊겼어요. 연결한 앱에서 다시 연결해 주세요.", detail: "" };
  }
  if (session.last_error_code === "server_restored") {
    return { summary: "서버가 복구돼 세션을 다시 확인해야 해요. 중지한 뒤 재개해 주세요.", detail: "" };
  }
  if (session.last_error_code === "runtime_start_failed") {
    return { summary: "에이전트를 시작하지 못했어요. 다시 시도해 주세요.", detail: "" };
  }
  if (session.last_error_code === "runtime_stop_unconfirmed") {
    return { summary: "에이전트가 정말 끝났는지 확인하지 못했어요. 복구를 눌러 확인해 주세요.", detail: "" };
  }
  if (session.last_error_code === "runtime_start_recovered_gone") {
    return { summary: "시작 요청이 서버 복구 전에 끝났어요. 다시 시작해 주세요.", detail: "" };
  }
  if (session.last_error_code === "runtime_start_abandoned_before_effect") {
    return { summary: "시작 요청이 승인 전에 취소됐어요. 다시 시작해 주세요.", detail: "" };
  }
  if (session.last_error_code === "runtime_stop_abandoned_before_effect") {
    return { summary: "중지 요청이 승인 전에 취소됐어요. 다시 중지해 주세요.", detail: "" };
  }
  if (session.last_error_code === "runtime_profile_unsupported") {
    return { summary: "저장된 실행 설정을 지금은 사용할 수 없어요. 실행 설정에서 다시 선택해 주세요.", detail: "" };
  }
  const raw = session.last_error || "";
  if (!raw) return { summary: "", detail: "" };
  if (/[가-힣]/.test(raw)) return { summary: raw, detail: "" };
  const providerName = resolveProviderPresentation({ providerKind: session.provider_kind }).providerName || "에이전트";
  return { summary: `${providerName} 요청을 처리하지 못했어요.`, detail: raw };
}

export default function AgentSessionDetails({
  session,
  provider,
  localProviderActions = false,
  onControl,
  onConfigure,
  activityVisible = false,
  onActivityVisibilityChange,
}: {
  session: RoomAgentSession;
  provider?: NativeCliProviderAvailability;
  localProviderActions?: boolean;
  onControl?: (
    session: RoomAgentSession,
    action: AgentSessionControlAction
  ) => void | Promise<void>;
  onConfigure?: (
    session: RoomAgentSession,
    settings: Record<string, string>,
    fullAccessAcknowledged?: true
  ) => void | Promise<void>;
  activityVisible?: boolean;
  onActivityVisibilityChange?: (session: RoomAgentSession, visible: boolean) => void;
}) {
  const [pendingAction, setPendingAction] = useState<AgentSessionControlAction | null>(null);
  const [actionStatus, setActionStatus] = useState("");
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsBusy, setSettingsBusy] = useState(false);
  const status = session.runtime_status;
  const serverOwned = !session.external_owned && session.process_ownership === "server";
  const hasRunBefore = Boolean(
    session.turn_count ||
      session.last_seen_event_id
  );
  const canStart =
    serverOwned && !session.recovery_required && !hasRunBefore &&
    ["", "available", "stopped", "error", "disconnected"].includes(status || "");
  const canPause = agentSessionCanPause(session);
  const canStop = agentSessionIsPresent(status) || status === "error" || session.recovery_required;
  // A server killed without a clean shutdown leaves this: stop is the owner request that lets
  // the runtime prove the previous owner is gone, so it is offered as recovery.
  const strandedRuntime =
    session.recovery_required && session.last_error_code === "runtime_authority_uncertain";
  const canResume = agentSessionCanResume(session);
  const canInterrupt =
    (serverOwned ? provider?.turn_interrupt === "retained_runtime"
      : session.external_owned && session.process_ownership === "external" && session.external_retained_interrupt === true)
    && status === "busy" && !session.recovery_required;
  const canConfigure =
    serverOwned && !session.enabled && !session.recovery_required &&
    ["", "available", "stopped", "error", "disconnected"].includes(status || "");
  const runtimeSettingLabels =
    (provider?.controls || []).map((control) => control.label).join("·") || "실행 설정";
  const invalidRuntimeControl = provider?.controls.find((control) =>
    !effectiveProviderControlOptions(provider, control, settings).some(
      (option) => option.value === (settings[control.key] ?? "")
    )
  );
  const invalidRuntimeValue = invalidRuntimeControl
    ? settings[invalidRuntimeControl.key] || ""
    : "";
  const sessionError = sessionErrorInfo(session);
  const storedSettings = useMemo(() => {
    const storedModel = session.model || controlDefault(provider, "model");
    return {
      model:
        provider && storedModel
          ? canonicalProviderModelValue(provider, storedModel)
          : storedModel,
      reasoning_effort:
        session.reasoning_effort ?? controlDefault(provider, "reasoning_effort"),
      service_tier:
        session.service_tier ?? controlDefault(provider, "service_tier"),
      variant: session.variant ?? controlDefault(provider, "variant"),
      execution_harness:
        session.execution_harness || controlDefault(provider, "execution_harness") || "builtin",
      permission_mode:
        session.permission_mode || controlDefault(provider, "permission_mode") || "meeting_read_only",
      max_output_tokens:
        String(session.max_output_tokens || "") ||
        controlDefault(provider, "max_output_tokens"),
    };
  }, [
    provider,
    session.session_id,
    session.model,
    session.reasoning_effort,
    session.service_tier,
    session.variant,
    session.execution_harness,
    session.permission_mode,
    session.max_output_tokens,
  ]);

  useEffect(() => { setSettings(provider && requiresOpenCodeApproval(provider, storedSettings) && storedSettings.permission_mode !== "full_access"
    ? { ...storedSettings, permission_mode: "workspace_write" } : storedSettings); }, [provider, storedSettings]);
  const settingsChanged = Object.entries(storedSettings).some(([key, value]) => settings[key] !== value);

  async function runControl(action: AgentSessionControlAction) {
    if (!onControl || pendingAction) return;
    setPendingAction(action);
    setActionStatus("");
    try {
      await onControl(session, action);
      setActionStatus(actionCompletedLabel(action));
    } catch (error) {
      setActionStatus(userFacingError(error, "세션 제어 요청 실패"));
    } finally {
      setPendingAction(null);
    }
  }

  async function saveSettings() {
    if (
      !onConfigure ||
      !canConfigure ||
      !settingsChanged ||
      invalidRuntimeControl ||
      settingsBusy
    ) return;
    setSettingsBusy(true);
    setActionStatus("");
    try {
      if (settings.permission_mode === "full_access") await onConfigure(session, settings, true);
      else await onConfigure(session, settings);
      // The saved form state is the only feedback; no success box.
      setActionStatus("");
    } catch (error) {
      setActionStatus(userFacingError(error, "실행 설정을 저장하지 못했어요."));
    } finally {
      setSettingsBusy(false);
    }
  }

  function updateRuntimeSetting(key: string, value: string) {
    if (!provider) return;
    setSettings((previous) => ({
      ...previous,
      ...reconcileProviderSettings(
        provider,
        {
          ...previous,
          [key]: value,
        },
        key
      ),
    }));
  }

  return (
    <section className="dc-member-detail-section" aria-label={`${session.display_name} 실행 및 설정`}>
      <div className="dc-member-detail-section-heading">
        <h3>실행 및 설정</h3>
        <span className="dc-agent-session-state" data-state={agentSessionPresenceStatus(status, session.recovery_required)}>
          {agentSessionStatusLabel(status, session.recovery_required)}
        </span>
      </div>
      {(status === "error" || session.recovery_required) && sessionError.summary && (
        <div className="dc-room-play-error preserve-words">
          <p>{sessionError.summary}{sessionError.status && <span className="dc-room-play-error-code"> ({sessionError.status})</span>}</p>
          {sessionError.detail && <details><summary>자세히</summary><p className="break-all whitespace-pre-wrap">{sessionError.detail}</p></details>}
        </div>
      )}
      {onControl && (
        <div className="dc-member-session-actions" aria-label={`${session.display_name} 세션 제어`}>
          {(canStart || pendingAction === "start") && <button
            type="button"
            className="dc-member-session-button"
            title="세션 시작"
            disabled={!canStart || Boolean(pendingAction)}
            onClick={() => void runControl("start")}
          >
            <Play size={15} />
            시작
          </button>}
          {(canPause || pendingAction === "pause") && <button
            type="button"
            className="dc-member-session-button"
            data-variant="quiet"
            title="세션 일시정지"
            disabled={!canPause || Boolean(pendingAction)}
            onClick={() => void runControl("pause")}
          >
            <CirclePause size={15} />
            일시정지
          </button>}
          {(canStop || pendingAction === "stop") && <button
            type="button"
            className="dc-member-session-button"
            data-variant={strandedRuntime ? "quiet" : "danger"}
            title={strandedRuntime ? "남은 실행이 끝났는지 확인하고 정리해요" : "세션 중지"}
            disabled={!canStop || Boolean(pendingAction)}
            onClick={() => void runControl("stop")}
          >
            {strandedRuntime ? <LifeBuoy size={15} /> : <Square size={15} />}
            {strandedRuntime ? "복구" : "중지"}
          </button>}
          {(canResume || pendingAction === "resume") && <button
            type="button"
            className="dc-member-session-button"
            title="세션 재개"
            disabled={!canResume || Boolean(pendingAction)}
            onClick={() => void runControl("resume")}
          >
            <RotateCcw size={15} />
            재개
          </button>}
          {(canInterrupt || pendingAction === "interrupt") && <button
            type="button"
            className="dc-member-session-button"
            data-variant="quiet"
            title="현재 응답 중단"
            disabled={!canInterrupt || Boolean(pendingAction)}
            onClick={() => void runControl("interrupt")}
          >
            <Zap size={15} />
            응답 중단
          </button>}
        </div>
      )}
      {!serverOwned && (
        <p className="preserve-words">외부에서 실행하는 에이전트예요. 시작과 실행 설정은 연결한 앱에서 관리해 주세요.</p>
      )}
      {serverOwned && provider && onConfigure && (
        <details aria-label={`${session.display_name} 실행 설정`}
          onToggle={(event) => setSettingsOpen(event.currentTarget.open)}>
          <summary>실행 설정</summary>
          <div className="dc-agent-runtime-settings">
          {settingsOpen && <div style={{ gridColumn: "1 / -1" }}><ProviderModelRefresh title="모델" providerId={provider.id}
            localAvailable={localProviderActions}
            automaticAllowed={provider.discovery_error_code !== "authentication_required"} /></div>}
          {requiresOpenCodeApproval(provider, settings) && (
            <p className="preserve-words" style={{ gridColumn: "1 / -1", color: "var(--color-text-muted)", fontSize: 12 }}>{OPENCODE_FREE_PERMISSION_HINT}</p>
          )}
          {displayProviderControls(provider).map((control) => {
            const providerSupportsControl = provider.controls.some(
              (candidate) => candidate.key === control.key
            );
            const options = providerSupportsControl
              ? effectiveProviderControlOptions(provider, control, settings)
              : control.options;
            return (
              <ProviderRuntimeSettingField
                key={`${session.session_id}:${control.key}`}
                control={control}
                options={options}
                value={
                  providerSupportsControl
                    ? settings[control.key] ?? control.default_value
                    : control.default_value
                }
                disabled={!canConfigure || settingsBusy || !providerSupportsControl}
                onChange={(value) => updateRuntimeSetting(control.key, value)}
              />
            );
          })}
          <button
            type="button"
            className="dc-member-session-button"
            // The settings grid stretches its own row, so keep the action at its label width.
            style={{ justifySelf: "start" }}
            disabled={
              !canConfigure ||
              !settingsChanged ||
              Boolean(invalidRuntimeControl) ||
              settingsBusy
            }
            onClick={() => void saveSettings()}
          >
            <Save size={15} />
            실행 설정 저장
          </button>
          <p className="preserve-words">
            {!canConfigure
              ? "현재 실행 설정이에요. 변경하려면 에이전트를 중지해 주세요."
              : invalidRuntimeControl
                ? invalidRuntimeValue
                  ? `저장값: “${invalidRuntimeValue}”. 현재 선택 가능한 ${invalidRuntimeControl.label} 목록에 없어요.`
                  : `${invalidRuntimeControl.label}을(를) 선택하세요.`
                : `${runtimeSettingLabels}을 함께 저장해요. 변경은 다음 세션 시작부터 적용돼요.`}
          </p>
          </div>
        </details>
      )}
      {serverOwned && provider && onConfigure && (
        <AgentSessionPersonaSettings
          session={session}
          provider={provider}
          canConfigure={canConfigure}
          onConfigure={onConfigure}
          onStatus={setActionStatus}
        />
      )}
      <AgentActivitySettings
        session={session}
        activityVisible={activityVisible}
        onActivityVisibilityChange={onActivityVisibilityChange}
      />
      {actionStatus && <p role="status" className="dc-member-session-status preserve-words">{actionStatus}</p>}
    </section>
  );
}

function controlDefault(provider: NativeCliProviderAvailability | undefined, key: string) {
  return provider?.controls?.find((control) => control.key === key)?.default_value || "";
}
