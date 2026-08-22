import { FormEvent, useMemo, useState } from "react";
import type { RoomAgentSession } from "./api";
import type {
  NativeCliProviderAvailability,
  ProviderCatalogSnapshot,
} from "./roomSocketTypes";

type ControlValues = Record<string, string>;

interface AgentSessionPanelProps {
  catalog: ProviderCatalogSnapshot;
  online: boolean;
  sessions: RoomAgentSession[];
  onCreate: (payload: Record<string, unknown>) => Promise<void>;
}

export function defaultControlValues(
  provider: NativeCliProviderAvailability | undefined
): ControlValues {
  return Object.fromEntries(
    (provider?.controls || []).map((control) => [control.key, control.default_value])
  );
}

export function optionsForControl(
  provider: NativeCliProviderAvailability,
  controlKey: string,
  model: string
) {
  const control = provider.controls.find((candidate) => candidate.key === controlKey);
  if (!control || (controlKey !== "reasoning_effort" && controlKey !== "service_tier")) {
    return control?.options || [];
  }
  const relationKey = controlKey === "reasoning_effort"
    ? "reasoning_efforts"
    : "service_tiers";
  const modelOption = provider.controls
    .find((candidate) => candidate.key === "model")
    ?.options.find((option) => option.value === model);
  const relation = modelOption?.metadata?.[relationKey];
  if (!Array.isArray(relation)) return control.options;
  return control.options.filter((option) =>
    option.value === "" || option.value === "default" || relation.includes(option.value)
  );
}

export function agentCreationPayload(
  catalog: ProviderCatalogSnapshot,
  providerId: string,
  displayName: string,
  workspace: string,
  controls: ControlValues
): Record<string, unknown> {
  return {
    provider_id: providerId,
    catalog_revision: catalog.catalog_revision,
    display_name: displayName.trim(),
    workspace: workspace.trim(),
    start_now: false,
    ...controls,
  };
}

export default function AgentSessionPanel({
  catalog,
  online,
  sessions,
  onCreate,
}: AgentSessionPanelProps) {
  const startable = useMemo(
    () => catalog.providers.filter((provider) =>
      provider.available && provider.startable && provider.discovery_status === "ready"
    ),
    [catalog.providers]
  );
  const [providerId, setProviderId] = useState(startable[0]?.id || "");
  const selectedProvider = startable.find((provider) => provider.id === providerId);
  const [controlValues, setControlValues] = useState<ControlValues>(() =>
    defaultControlValues(selectedProvider)
  );
  const [displayName, setDisplayName] = useState("");
  const [workspace, setWorkspace] = useState("");
  const [creating, setCreating] = useState(false);
  const [result, setResult] = useState("");

  function chooseProvider(nextId: string) {
    const provider = startable.find((candidate) => candidate.id === nextId);
    setProviderId(nextId);
    setControlValues(defaultControlValues(provider));
    setResult("");
  }

  function chooseControl(key: string, value: string) {
    setControlValues((current) => {
      const next = { ...current, [key]: value };
      if (key !== "model" || !selectedProvider) return next;
      for (const relatedKey of ["reasoning_effort", "service_tier"]) {
        const options = optionsForControl(selectedProvider, relatedKey, value);
        if (!options.some((option) => option.value === next[relatedKey])) {
          const related = selectedProvider.controls.find((control) => control.key === relatedKey);
          const preferred = options.find((option) => option.value === related?.default_value);
          next[relatedKey] = preferred?.value || options[0]?.value || "";
        }
      }
      return next;
    });
  }

  async function create(event: FormEvent) {
    event.preventDefault();
    if (!selectedProvider || creating || !displayName.trim() || !workspace.trim()) return;
    setCreating(true);
    setResult("");
    try {
      await onCreate(agentCreationPayload(
        catalog,
        selectedProvider.id,
        displayName,
        workspace,
        controlValues
      ));
      setDisplayName("");
      setResult("중지된 Agent Session을 정본 방에 추가했습니다.");
    } catch (error) {
      setResult(error instanceof Error ? error.message : "Agent Session을 추가하지 못했습니다.");
    } finally {
      setCreating(false);
    }
  }

  return (
    <section className="agent-panel" aria-labelledby="agent-panel-title">
      <div className="agent-panel__heading">
        <div>
          <p className="eyebrow">AGENT SESSIONS</p>
          <h2 id="agent-panel-title">에이전트 추가</h2>
        </div>
        <span className={`catalog-state catalog-state--${catalog.status}`}>
          catalog · {catalog.status}
        </span>
      </div>

      <div className="provider-strip" aria-label="발견된 provider">
        {catalog.providers.map((provider) => (
          <span key={provider.id} className={provider.available ? "provider-ready" : "provider-failed"}>
            {provider.display_name} · {provider.discovery_status}
          </span>
        ))}
      </div>

      <form className="agent-form" onSubmit={create}>
        <label>
          Provider
          <select
            aria-label="Provider"
            disabled={!online || creating || startable.length === 0}
            onChange={(event) => chooseProvider(event.target.value)}
            value={providerId}
          >
            {startable.length === 0 && <option value="">사용 가능한 provider 없음</option>}
            {startable.map((provider) => (
              <option key={provider.id} value={provider.id}>{provider.display_name}</option>
            ))}
          </select>
        </label>
        <label>
          표시 이름
          <input
            aria-label="Agent 표시 이름"
            disabled={!online || creating}
            maxLength={64}
            onChange={(event) => setDisplayName(event.target.value)}
            placeholder="예: Terra"
            value={displayName}
          />
        </label>
        <label className="agent-form__wide">
          작업공간
          <input
            aria-label="Agent 작업공간"
            disabled={!online || creating}
            onChange={(event) => setWorkspace(event.target.value)}
            placeholder="이미 존재하는 절대 경로"
            value={workspace}
          />
        </label>
        {selectedProvider?.controls.map((control) => {
          const options = optionsForControl(
            selectedProvider,
            control.key,
            controlValues.model || selectedProvider.default_model
          );
          return (
            <label key={control.key}>
              {control.label}
              {control.kind === "combobox" ? (
                <>
                  <input
                    aria-label={control.label}
                    disabled={!online || creating}
                    list={`agent-${selectedProvider.id}-${control.key}`}
                    onChange={(event) => chooseControl(control.key, event.target.value)}
                    value={controlValues[control.key] || ""}
                  />
                  <datalist id={`agent-${selectedProvider.id}-${control.key}`}>
                    {options.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
                  </datalist>
                </>
              ) : (
                <select
                  aria-label={control.label}
                  disabled={!online || creating}
                  onChange={(event) => chooseControl(control.key, event.target.value)}
                  value={controlValues[control.key] || ""}
                >
                  {options.map((option) => (
                    <option key={option.value || "default-empty"} value={option.value}>{option.label}</option>
                  ))}
                </select>
              )}
            </label>
          );
        })}
        <button
          className="agent-form__submit"
          disabled={!online || creating || !selectedProvider || !displayName.trim() || !workspace.trim()}
          type="submit"
        >
          {creating ? "추가 중…" : "중지 상태로 추가"}
        </button>
      </form>
      {result && <p className="agent-result" role="status">{result}</p>}

      <div className="agent-roster" data-testid="agent-roster">
        {sessions.length === 0 ? (
          <p>아직 등록된 Agent Session이 없습니다.</p>
        ) : sessions.map((session) => (
          <article key={session.session_id}>
            <div>
              <strong>{session.display_name}</strong>
              <span>{session.provider_kind} · {session.model}</span>
            </div>
            <span className="runtime-state">{session.runtime_status}</span>
          </article>
        ))}
      </div>
    </section>
  );
}
