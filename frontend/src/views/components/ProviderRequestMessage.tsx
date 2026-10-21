import { useEffect, useRef, useState } from "react";
import type { PendingProviderRequest } from "../../types/generated/PendingProviderRequest";
import type { ProviderRequestKind } from "../../types/generated/ProviderRequestKind";
import type { ProviderRequestOption } from "../../types/generated/ProviderRequestOption";
import type { ProviderRequestQuestion } from "../../types/generated/ProviderRequestQuestion";
import type { ProviderRequestResolution } from "../../types/generated/ProviderRequestResolution";
import { RoomSocketSayError, type RoomSocketHandle } from "../../roomSocketTypes";
import { resolveProviderPresentation } from "./providerBranding";
import { userFacingError } from "../../lib/apiErrors";

const controlStyle = { minHeight: 44, padding: "8px 12px", borderRadius: 6, border: "1px solid var(--color-panel-separator)", background: "var(--color-panel)", color: "var(--color-text-primary)" } as const;
const terminalLabels: Record<string, string> = {
  resolved: "응답을 전달했어요.", failed: "응답을 전달하지 못했어요. 에이전트에서 새 요청이 필요해요.",
  cancelled: "요청이 취소됐어요.", expired: "응답 시간이 만료됐어요.",
};
// Option labels come from the structured option kind, never the provider's English label.
// An unknown kind (a newer host may add one) still renders a neutral Korean label.
const optionKindLabels: Record<string, string> = {
  allow_once: "이번만 허용", allow_turn: "이번 차례는 모두 허용", allow_always: "앞으로도 허용", allow_session: "이번 세션 동안 허용",
  reject_once: "거부", reject_always: "항상 거부",
  deny: "거부", decline: "거부하고 계속", cancel: "거부하고 중단", allow_policy: "규칙을 추가하고 허용",
  deny_policy: "규칙을 추가하고 거부",
};
const neutralOptionLabel = "선택";

function providerNameOf(providerKind?: string): string {
  return resolveProviderPresentation({ providerKind }).providerName || "에이전트";
}

function requestHeadline(kind: ProviderRequestKind, providerName: string): string {
  if (kind === "permission") return `${providerName} 실행 권한 요청`;
  if (kind === "user_input") return `${providerName} 질문`;
  if (kind === "external_action") return `${providerName} 확인 요청`;
  return `${providerName} 요청`;
}

export function providerRequestOptionLabel(option: ProviderRequestOption): string {
  return optionKindLabels[option.kind] || neutralOptionLabel;
}

export function remainingLabel(expiresAt: string, now: number): string {
  const seconds = Math.ceil((Date.parse(expiresAt) - now) / 1000);
  if (!(seconds > 0)) return "시간이 지났어요";
  return seconds < 60 ? `${seconds}초 남음` : `${Math.ceil(seconds / 60)}분 남음`;
}

// Ticks only while a request is waiting on screen, so its remaining time stays true.
function useNow(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  return now;
}

export default function ProviderRequestMessage({ entry, state, socket, connected, canPost, providerKind }: {
  entry?: PendingProviderRequest; title?: string; state?: string; providerKind?: string;
  socket: RoomSocketHandle | null; connected: boolean; canPost: boolean;
}) {
  const providerName = providerNameOf(providerKind);
  return <section aria-label="에이전트 요청" style={{ marginTop: 8, maxWidth: 560, borderLeft: "2px solid var(--color-panel-separator)", padding: "4px 0 4px 12px", fontSize: 14, overflowWrap: "anywhere" }}>
    {entry ? <>
      {!connected && <p role="status">연결을 복구하고 있어요. 연결되면 응답할 수 있어요.</p>}
      <RequestForm key={entry.request.provider_request_id} entry={entry} socket={socket} enabled={connected && canPost} providerName={providerName} providerKind={providerKind} />
    </> : <><p className="font-semibold">{`${providerName} 요청`}</p><p role="status">{terminalLabels[state || ""] || "현재 응답할 수 없는 요청이에요."}</p></>}
  </section>;
}

function RequestForm({ entry, socket, enabled, providerName, providerKind }: { entry: PendingProviderRequest; socket: RoomSocketHandle | null; enabled: boolean; providerName: string; providerKind?: string }) {
  const { request, state } = entry;
  const [answers, setAnswers] = useState<Record<string, string[]>>({});
  const now = useNow();
  const [busy, setBusy] = useState(false);
  const [accepted, setAccepted] = useState(false);
  const [error, setError] = useState("");
  const attempt = useRef<ProviderRequestResolution | null>(null);
  const inFlight = useRef(false);
  const blocked = !enabled || !socket || busy || accepted || state === "resolving";
  const locked = blocked || attempt.current !== null;
  const complete = request.prompt.response_kind === "option" ? false :
    request.prompt.response_kind === "answers" ? request.prompt.questions.every((question) => (answers[question.id]?.length ?? 0) > 0) : true;
  async function submit(chosen?: ProviderRequestResolution) {
    if (blocked || (!chosen && !complete && !attempt.current) || inFlight.current || !socket) return;
    const resolution: ProviderRequestResolution = attempt.current ?? chosen ?? (
      request.prompt.response_kind === "answers" ? { response_kind: "answers", answers } : { response_kind: "acknowledge" }
    );
    inFlight.current = true; attempt.current = resolution; setBusy(true); setError("");
    try {
      await socket.resolveProviderRequest(request.provider_request_id, resolution);
      setAccepted(true);
      setAnswers({}); attempt.current = null;
    } catch (failure) {
      const unknown = failure instanceof RoomSocketSayError && failure.category === "outcome_unknown";
      setError(unknown ? "응답 결과를 확인하지 못했어요. 같은 응답으로 다시 확인할 수 있어요." : userFacingError(failure, "응답을 보내지 못했어요."));
      if (!unknown) attempt.current = null;
    } finally { inFlight.current = false; setBusy(false); }
  }
  const options = request.prompt.response_kind === "option" ? request.prompt.options.filter((option) => !(providerKind === "opencode_server" && request.request_kind === "permission" && (option.id === "always" || option.kind === "allow_always"))) : [];
  const primary = options.find((option) => option.kind === "allow_once") ?? options[0];
  // What a lasting grant covers is shown, not left to a tooltip phones never show.
  const lasting = options.find((option) => option.kind === "allow_always" && option.description);
  const lastingId = `${request.provider_request_id}-lasting`;
  return <form onSubmit={(event) => { event.preventDefault(); void submit(); }} style={{ display: "grid", gap: 8 }}>
    <div><p><strong>{requestHeadline(request.request_kind, providerName)}</strong>
      <span style={{ color: "var(--color-text-muted)" }}> · <time dateTime={entry.expires_at}>{remainingLabel(entry.expires_at, now)}</time></span></p>
      {request.description && <p style={{ whiteSpace: "pre-wrap" }}>{request.description}</p>}</div>
    {options.length > 0 && <div role="group" aria-label="응답" style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
      {options.map((option) => <button key={option.id} type="button" disabled={locked} title={option.description || undefined}
        aria-describedby={option === lasting ? lastingId : undefined}
        className={`dc-button ${option === primary ? "dc-button--primary" : "dc-button--secondary"}`}
        onClick={() => void submit({ response_kind: "option", option_id: option.id })}>{providerRequestOptionLabel(option)}</button>)}
    </div>}
    {lasting && <p id={lastingId} style={{ color: "var(--color-text-muted)", fontSize: 12 }}>앞으로도 허용: {lasting.description}</p>}
    {request.prompt.response_kind !== "option" && <fieldset disabled={locked} style={{ display: "grid", gap: 8, minWidth: 0 }}>
      {request.prompt.response_kind === "answers" && request.prompt.questions.map((question) => <QuestionInput key={question.id}
        question={question} values={answers[question.id] ?? []} group={`${request.provider_request_id}-${question.id}`}
        onChange={(value) => setAnswers((previous) => ({ ...previous, [question.id]: value }))} />)}
      {request.prompt.response_kind === "acknowledge" && request.prompt.action_url &&
        <a href={request.prompt.action_url} target="_blank" rel="noopener noreferrer" className="dc-button dc-button--secondary" style={{ justifySelf: "start" }}>요청 페이지 열기</a>}
    </fieldset>}
    {error && <p role="alert">{error}</p>}
    {accepted || state === "resolving" ? <p role="status">응답을 접수했어요. 에이전트로 전달한 결과를 기다리고 있어요.</p> :
      (attempt.current || request.prompt.response_kind !== "option") &&
      <button type="submit" className="dc-button dc-button--primary" disabled={blocked || (!complete && !attempt.current)} style={{ justifySelf: "start" }}>
        {busy ? "응답 전송 중…" : attempt.current ? "같은 응답으로 다시 확인" : request.prompt.response_kind === "acknowledge" ? "작업 완료 알리기" : "응답 보내기"}
      </button>}
  </form>;
}

function QuestionInput({ question, values, onChange, group }: {
  question: ProviderRequestQuestion; values: string[]; onChange: (value: string[]) => void; group: string;
}) {
  const labels = question.options.map((option) => option.label);
  const other = values.find((value) => !labels.includes(value)) ?? "";
  return <fieldset style={{ display: "grid", gap: 8, minWidth: 0 }}>
    <legend style={{ marginBottom: 8 }}>{question.header}: {question.question}</legend>
    {question.options.map((option) => <label key={option.id} style={{ ...controlStyle, display: "flex", alignItems: "center", gap: 12 }}>
      <input type={question.multiple ? "checkbox" : "radio"} name={group} checked={values.includes(option.label)}
        onChange={(event) => onChange(question.multiple ? event.target.checked ? [...values, option.label] : values.filter((value) => value !== option.label) : [option.label])} />
      <span>{option.label}{option.description && <small style={{ display: "block" }}>{option.description}</small>}</span>
    </label>)}
    {(question.is_other || !question.options.length) && <label style={{ display: "grid", gap: 8 }}>
      {question.options.length ? "직접 입력" : "답변"}
      <input className="dc-input w-full" type={question.is_secret ? "password" : "text"} autoComplete="off" value={other} required={!question.options.length}
        onChange={(event) => {
          const selected = question.multiple ? values.filter((value) => labels.includes(value)) : [];
          onChange(event.target.value ? [...selected, event.target.value] : selected);
        }} />
    </label>}
  </fieldset>;
}
