import { useCallback, useEffect, useRef, useState } from "react";
import { createLocalAttendee, fetchLocalAttendee, commandLocalAttendee, localAttendeeCreateRequest } from "../../api/localAttendee";
import { fetchLocalProviderCatalog } from "../../api/providerOperations";
import { ApiError } from "../../lib/apiErrors";
import { requestDesktopHostProductSurface } from "../../lib/desktopBridge";
import { LOCAL_ATTENDEE_PHASE_LABELS, readLocalAttendeeHandoff } from "../../lib/localAttendee";
import type { FrontendLiveAgentCreateRequest } from "../../api";
import type { AttendeeEntryPacket } from "../../types/generated/AttendeeEntryPacket";
import type { LocalAttendeeCreate } from "../../types/generated/LocalAttendeeCreate";
import type { LocalAttendeeStatus } from "../../types/generated/LocalAttendeeStatus";
import type { LocalAttendeeAction } from "../../types/generated/LocalAttendeeAction";
import type { ProviderCatalog } from "../../types/generated/ProviderCatalog";
import AgentCreateModal from "./AgentCreateModal";

export default function LocalAttendeePanel() {
  const [handoff] = useState(() => {
    try { return { packet: readLocalAttendeeHandoff(new URL(window.location.href)), error: "" }; }
    catch { return { packet: null, error: "초대를 읽지 못했어요. 브라우저에서 에이전트 추가 화면을 다시 열어 주세요." }; }
  });
  return handoff.packet ? <LocalCreation packet={handoff.packet} />
    : <main style={{ padding: 24 }}><p role="alert">{handoff.error}</p></main>;
}

function LocalCreation({ packet }: { packet: AttendeeEntryPacket }) {
  const [catalog, setCatalog] = useState<ProviderCatalog | null>(null);
  const [operation, setOperation] = useState<LocalAttendeeStatus | null>(null);
  const [editable, setEditable] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState("");
  // Exact submitted input survives transport loss; it is never rebuilt from an edited draft.
  const submitted = useRef<LocalAttendeeCreate | null>(null);
  const active = useRef(false);
  // Cancel supersedes the displayed result of an older HTTP waiter, not its task.
  const generation = useRef(0);
  const cancelIntent = useRef(false);
  const read = useCallback(async (signal?: AbortSignal) => {
    const observation = ++generation.current;
    const current = () => !signal?.aborted && observation === generation.current;
    setBusy(true);
    setError("");
    try {
      await requestDesktopHostProductSurface();
      const localCatalog = await fetchLocalProviderCatalog(signal);
      if (!current()) return;
      setCatalog(localCatalog);
      try {
        const status = await fetchLocalAttendee(packet, signal);
        if (current()) { setOperation(status); setEditable(false); }
      } catch (failure) {
        if (!(failure instanceof ApiError && failure.code === "local_attendee_missing")) throw failure;
        if (current()) {
          const expired = Date.parse(packet.expires_at) <= Date.now();
          setEditable(submitted.current === null && !expired);
          if (expired) setError("초대가 만료됐어요. 브라우저에서 새 초대를 만들어 주세요.");
        }
      }
    } catch (failure) {
      if (current()) setError(failure instanceof Error ? failure.message : "참가 상태를 확인하지 못했어요.");
    } finally { if (current()) setBusy(false); }
  }, [packet]);
  useEffect(() => {
    active.current = true;
    const controller = new AbortController();
    void read(controller.signal);
    return () => { active.current = false; generation.current += 1; controller.abort(); };
  }, [read]);

  async function create(request?: FrontendLiveAgentCreateRequest) {
    if (cancelIntent.current) return;
    const input = submitted.current ?? (request && localAttendeeCreateRequest(packet, request));
    if (!input) return;
    const observation = ++generation.current;
    const current = () => active.current && observation === generation.current;
    submitted.current = input;
    setEditable(false);
    setBusy(true);
    setError("");
    try {
      const result = await createLocalAttendee(packet, input);
      if (current()) setOperation(result);
    } catch (failure) {
      // Only an explicit rejection plus the owner's missing record permits editing again.
      if (current() && failure instanceof ApiError && failure.status === 409) {
        try { const status = await fetchLocalAttendee(packet); if (current()) setOperation(status); }
        catch (readFailure) {
          if (current() && readFailure instanceof ApiError && readFailure.code === "local_attendee_missing") {
            submitted.current = null;
            setEditable(true);
          }
        }
      }
      if (current()) setError(failure instanceof Error ? failure.message : "참가 결과를 확인하지 못했어요.");
      throw failure;
    } finally { if (current()) setBusy(false); }
  }

  async function command(action: LocalAttendeeAction) {
    const cancel = action === "cancel";
    if (cancelling || (!cancel && (busy || cancelIntent.current))) return;
    const observation = ++generation.current;
    const current = () => active.current && observation === generation.current;
    if (cancel) { cancelIntent.current = true; setCancelling(true); }
    else setBusy(true);
    setError("");
    try { const result = await commandLocalAttendee(packet, action); if (current()) setOperation(result); }
    catch (failure) { if (current()) setError(failure instanceof Error ? failure.message : "참가 상태를 확인하지 못했어요."); }
    finally { if (current()) { setBusy(false); setCancelling(false); } }
  }
  const roomLabel = `${new URL(packet.join_url).hostname} · ${packet.room_id}`;
  return <main style={{ padding: 24, maxWidth: 680, margin: "0 auto", display: "grid", gap: 20 }}>
    <header><h1 className="text-2xl font-black text-text-primary">내 PC에서 에이전트 추가</h1>
      <p className="dc-agent-hint preserve-words">{roomLabel}</p></header>
    <p role="status">{cancelling ? "에이전트 종료와 방 나가기를 확인하고 있어요." : busy ? "참가 상태를 확인하고 있어요." : operation
      ? LOCAL_ATTENDEE_PHASE_LABELS[operation.phase] : editable ? "이 PC에서 사용할 제공자 설정을 선택해 주세요."
      : submitted.current ? "응답을 받지 못했어요. 상태를 확인하거나 같은 요청으로 다시 시도해 주세요." : "참가 상태를 먼저 확인해 주세요."}</p>
    {error && <p role="alert">{error}</p>}
    {operation?.error_code && <p className="dc-agent-hint preserve-words">{operation.error_code === "local_attendee_process_restarted"
      ? "앱을 다시 시작해 이전 실행 결과를 확인할 수 없어요. 방에서 이전 참가자를 정리한 뒤 새 초대를 만들어 주세요." : operation.error_code}</p>}
    <div style={{ display: "flex", flexWrap: "wrap", gap: 12 }}>
      <button className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }} disabled={busy || cancelling} onClick={() => void read()}>상태 다시 확인</button>
      {editable && dismissed && <button className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }} onClick={() => setDismissed(false)}>설정 계속하기</button>}
      {!operation && submitted.current && !cancelIntent.current && <button className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }} disabled={busy || cancelling}
        onClick={() => { void create().catch(() => undefined); }}>같은 요청 다시 시도</button>}
      {!cancelIntent.current && operation?.phase === "admitted" && <button className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }} disabled={busy || cancelling} onClick={() => void command("start")}>이 PC에서 실행</button>}
      {!cancelIntent.current && operation?.phase === "admission_unresolved" && <button className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }} disabled={busy || cancelling} onClick={() => void command("retry_admission")}>참가 결과 복구</button>}
      {(operation || submitted.current) && !["stopped", "failed", "cleanup_unconfirmed"].includes(operation?.phase ?? "") && <button className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }} disabled={cancelling}
        onClick={() => void command("cancel")}>에이전트 종료하고 나가기</button>}
    </div>
    {catalog && (editable || submitted.current) && !operation && <div style={{ display: editable && !dismissed ? undefined : "none" }}>
      <AgentCreateModal open meetingId={packet.room_id} roomLabel={roomLabel}
        providers={catalog.providers.filter((provider) => provider.id === packet.provider)} catalogRevision={catalog.catalog_revision}
        initialSelection={{ providerId: packet.provider, displayName: packet.display_name }} onCatalogChange={setCatalog}
        onCreate={create} onClose={() => setDismissed(true)} />
    </div>}
  </main>;
}
