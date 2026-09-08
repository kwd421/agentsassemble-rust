import { useLayoutEffect, useRef, useState } from "react";
import type { RoomEvent } from "../../api";
import type { PendingProviderRequest } from "../../types/generated/PendingProviderRequest";
import type { ProviderRequestQuestion } from "../../types/generated/ProviderRequestQuestion";
import type { ProviderRequestResolution } from "../../types/generated/ProviderRequestResolution";
import { RoomSocketSayError, type RoomSocketHandle } from "../../roomSocketTypes";

const controlStyle = { minHeight: 44, padding: "8px 12px", borderRadius: 6, border: "1px solid var(--color-text-muted)" } as const;
const terminalLabels: Record<string, string> = {
  resolved: "응답을 전달했어요.", failed: "응답을 전달하지 못했어요. 에이전트에서 새 요청이 필요해요.",
  cancelled: "요청이 취소됐어요.", expired: "응답 시간이 만료됐어요.",
};

export default function ProviderRequestsPanel({ requests, socket, connected, canPost, events }: {
  requests: PendingProviderRequest[];
  socket: RoomSocketHandle | null;
  connected: boolean;
  canPost: boolean;
  events: RoomEvent[];
}) {
  const [open, setOpen] = useState(false);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const openerRef = useRef<HTMLButtonElement>(null);
  const lastClosed = events.filter((event) => event.type === "provider_request_closed").at(-1);
  useLayoutEffect(() => {
    if (!open) return;
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => { dialog?.close(); openerRef.current?.focus(); };
  }, [open]);
  if (!requests.length && !lastClosed) return null;
  return <section aria-label="에이전트 요청" style={{ padding: "8px 24px", borderBottom: "1px solid var(--color-panel-soft)", flexShrink: 0 }}>
    <button ref={openerRef} type="button" style={controlStyle} onClick={() => setOpen(true)}>
      에이전트 요청 {requests.length > 0 ? `(${requests.length})` : "결과"}
    </button>
    <dialog ref={dialogRef} className="dc-create-channel-modal" aria-label="에이전트 요청"
      style={{ display: open ? "block" : "none", position: "fixed", inset: 0, margin: "auto", width: "min(560px, calc(100vw - 32px))", maxHeight: "calc(100dvh - 32px)", padding: 0, color: "var(--color-text-primary)", overflowY: "auto" }}
      onCancel={(event) => { event.preventDefault(); setOpen(false); }}
      onClick={(event) => {
        if (event.target !== event.currentTarget) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) setOpen(false);
      }}>
      <header style={{ padding: "16px 24px", display: "flex", gap: 12, alignItems: "center" }}>
        <h2 style={{ flex: 1 }}>에이전트 요청</h2>
        <button type="button" style={controlStyle} onClick={() => setOpen(false)}>닫기</button>
      </header>
      <div style={{ padding: "0 24px 24px", display: "grid", gap: 24, overflowWrap: "anywhere" }}>
        {!connected && <p role="status">연결을 복구하고 있어요. 연결되면 응답할 수 있어요.</p>}
        {lastClosed && <p role="status">{requests.length > 0 && "이전 요청: "}{terminalLabels[String(lastClosed.state)]}</p>}
        {requests.map((entry) => <RequestForm key={entry.request.provider_request_id} entry={entry}
          socket={socket} enabled={connected && canPost} />)}
        {!requests.length && <p>대기 중인 요청이 없어요.</p>}
      </div>
    </dialog>
  </section>;
}

function RequestForm({ entry, socket, enabled }: { entry: PendingProviderRequest; socket: RoomSocketHandle | null; enabled: boolean }) {
  const { request, state } = entry;
  const [answers, setAnswers] = useState<Record<string, string[]>>({});
  const [optionId, setOptionId] = useState("");
  const [busy, setBusy] = useState(false);
  const [accepted, setAccepted] = useState(false);
  const [error, setError] = useState("");
  const attempt = useRef<ProviderRequestResolution | null>(null);
  const inFlight = useRef(false);
  const blocked = !enabled || !socket || busy || accepted || state === "resolving";
  const locked = blocked || attempt.current !== null;
  const complete = request.prompt.response_kind === "option" ? Boolean(optionId) :
    request.prompt.response_kind === "answers" ? request.prompt.questions.every((question) => (answers[question.id]?.length ?? 0) > 0) : true;
  async function submit() {
    if (blocked || (!complete && !attempt.current) || inFlight.current || !socket) return;
    const resolution: ProviderRequestResolution = attempt.current ?? (
      request.prompt.response_kind === "option" ? { response_kind: "option", option_id: optionId } :
      request.prompt.response_kind === "answers" ? { response_kind: "answers", answers } : { response_kind: "acknowledge" }
    );
    inFlight.current = true; attempt.current = resolution; setBusy(true); setError("");
    try {
      await socket.resolveProviderRequest(request.provider_request_id, resolution);
      setAccepted(true);
      setAnswers({}); setOptionId(""); attempt.current = null;
    } catch (failure) {
      const unknown = failure instanceof RoomSocketSayError && failure.category === "outcome_unknown";
      setError(unknown ? "응답 결과를 확인하지 못했어요. 같은 응답으로 다시 확인할 수 있어요." : failure instanceof Error ? failure.message : "응답을 보내지 못했어요.");
      if (!unknown) attempt.current = null;
    } finally { inFlight.current = false; setBusy(false); }
  }
  return <form onSubmit={(event) => { event.preventDefault(); void submit(); }} style={{ display: "grid", gap: 16 }}>
    <div><h3 style={{ fontWeight: 700 }}>{request.title}</h3><p style={{ whiteSpace: "pre-wrap" }}>{request.description}</p></div>
    <p>응답 기한: <time dateTime={entry.expires_at}>{new Date(entry.expires_at).toLocaleString("ko-KR")}</time></p>
    <fieldset disabled={locked} style={{ display: "grid", gap: 12, minWidth: 0 }}>
      {request.prompt.response_kind === "option" && request.prompt.options.map((option) => <label key={option.id} style={{ ...controlStyle, display: "flex", alignItems: "center", gap: 12 }}>
        <input type="radio" name={request.provider_request_id} required checked={optionId === option.id} onChange={() => setOptionId(option.id)} />
        <span>{option.label}{option.description && <small style={{ display: "block" }}>{option.description}</small>}</span>
      </label>)}
      {request.prompt.response_kind === "answers" && request.prompt.questions.map((question) => <QuestionInput key={question.id}
        question={question} values={answers[question.id] ?? []} group={`${request.provider_request_id}-${question.id}`}
        onChange={(value) => setAnswers((previous) => ({ ...previous, [question.id]: value }))} />)}
      {request.prompt.response_kind === "acknowledge" && request.prompt.action_url &&
        <a href={request.prompt.action_url} target="_blank" rel="noopener noreferrer" style={controlStyle}>요청 페이지 열기</a>}
    </fieldset>
    {error && <p role="alert">{error}</p>}
    {accepted || state === "resolving" ? <p role="status">응답을 접수했어요. 에이전트로 전달한 결과를 기다리고 있어요.</p> :
      <button type="submit" disabled={blocked || (!complete && !attempt.current)} style={{ ...controlStyle, background: "var(--color-accent)", justifySelf: "start" }}>
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
      <input type={question.is_secret ? "password" : "text"} autoComplete="off" value={other}
        style={{ ...controlStyle, width: "100%", minWidth: 0 }} required={!question.options.length}
        onChange={(event) => {
          const selected = question.multiple ? values.filter((value) => labels.includes(value)) : [];
          onChange(event.target.value ? [...selected, event.target.value] : selected);
        }} />
    </label>}
  </fieldset>;
}
