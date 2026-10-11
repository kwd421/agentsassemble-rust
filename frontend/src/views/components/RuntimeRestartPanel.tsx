import { useEffect, useRef, useState } from "react";
import { readRuntimeRestart, requestRuntimeRestart } from "../../api/runtimeRestart";
import type { RuntimeRestartPhase } from "../../types/generated/RuntimeRestartPhase";
import type { RuntimeRestartStatus } from "../../types/generated/RuntimeRestartStatus";

const LABELS: Record<RuntimeRestartPhase, string> = {
  quiescing: "새 작업을 멈추고 재시작을 준비하고 있어요.",
  draining: "연결을 정리하고 서버를 교체하고 있어요.",
  recovering: "세션을 복구하고 있어요.",
  completed: "재시작을 완료했어요.",
  failed: "재시작에 실패했어요. 자원 사용량을 확인한 뒤 다시 시도해 주세요.",
  aborted: "재시작 요청이 취소됐어요.",
};
const inProgress = (phase: RuntimeRestartPhase) => phase === "quiescing" || phase === "draining" || phase === "recovering";

export default function RuntimeRestartPanel() {
  const [status, setStatus] = useState<RuntimeRestartStatus | null>(null);
  const [query, setQuery] = useState<{ id?: string; revision: number }>({ revision: 0 });
  const [error, setError] = useState("");
  const [preparing, setPreparing] = useState(false);
  const [observing, setObserving] = useState(true);
  const request = useRef<AbortController | null>(null);
  useEffect(() => () => { request.current?.abort(); request.current = null; }, []);
  useEffect(() => {
    const controller = new AbortController();
    let disposed = false;
    let next = 0;
    let selected = query.id;
    setObserving(true);
    const deadline = window.setTimeout(() => {
      controller.abort();
      setObserving(false);
      setError("아직 결과를 확인하지 못했어요.");
    }, 60_000);
    const check = async () => {
      try {
        const value = await readRuntimeRestart(selected, controller.signal);
        if (disposed || controller.signal.aborted) return;
        setStatus(value);
        setError("");
        selected ??= value.operation?.operation_id;
        if ((value.operation && inProgress(value.operation.phase)) || (selected && !value.operation)) {
          next = window.setTimeout(() => { void check(); }, 1_000);
        } else {
          window.clearTimeout(deadline);
          setObserving(false);
        }
      } catch {
        if (disposed || controller.signal.aborted) return;
        setStatus(null);
        setError("재시작 결과를 확인하지 못했어요.");
        if (selected) next = window.setTimeout(() => { void check(); }, 1_000);
        else { window.clearTimeout(deadline); setObserving(false); }
      }
    };
    void check();
    return () => { disposed = true; controller.abort(); window.clearTimeout(next); window.clearTimeout(deadline); };
  }, [query]);

  const restart = async () => {
    if (request.current) return;
    const controller = new AbortController();
    request.current = controller;
    const operationId = query.id && !status?.operation ? query.id : crypto.randomUUID();
    let dispatched = false;
    const deadline = window.setTimeout(() => controller.abort(), 30_000);
    setPreparing(true); setError(""); setStatus(null);
    try {
      await requestRuntimeRestart(operationId, () => { dispatched = true; }, controller.signal);
      if (request.current !== controller) return;
      setObserving(true);
      setQuery((current) => ({ id: operationId, revision: current.revision + 1 }));
    } catch {
      if (request.current !== controller) return;
      setError(dispatched ? "요청 결과가 아직 확정되지 않았어요. 같은 작업의 결과를 확인하고 있어요." : "재시작 요청을 보내지 못했어요. 다시 시도해 주세요.");
      if (dispatched) {
        setObserving(true);
        setQuery((current) => ({ id: operationId, revision: current.revision + 1 }));
      }
    } finally {
      window.clearTimeout(deadline);
      if (request.current === controller) {
        request.current = null;
        setPreparing(false);
      }
    }
  };
  const receipt = status?.operation;
  const pending = observing || (receipt ? inProgress(receipt.phase) : false);
  return <section className="dc-computer-card" aria-label="서버 재시작">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="min-w-0">
        <h2 className="dc-computer-heading">서버 재시작</h2>
        <p className="mt-1 text-[13px] text-text-muted">진행 중인 대화가 끝난 뒤 시작해요. 연결이 잠시 끊겨요.</p>
      </div>
      <div className="flex shrink-0 flex-wrap gap-2">
      <button type="button" className="dc-quiet-button" disabled={!status?.supported || preparing || pending} onClick={() => { void restart(); }}>{preparing ? "요청 중" : query.id && !receipt ? "같은 요청 다시 보내기" : "서버 재시작"}</button>
      {(query.id || error) && <button type="button" className="dc-quiet-button" disabled={preparing || observing} onClick={() => {
        setError(""); setQuery((current) => ({ ...current, revision: current.revision + 1 }));
      }}>결과 확인</button>}
      </div>
    </div>
    {status && !status.supported && <p className="mt-3 text-[13px] text-text-muted">이 환경에서는 서버 재시작을 지원하지 않아요.</p>}
    {receipt && <p role="status" className="mt-3 text-[13px]">{LABELS[receipt.phase]}</p>}
    {observing && query.id && <p className="mt-3 text-[13px] text-text-muted">재시작 상태를 확인하고 있어요.</p>}
    {error && <p role="alert" className="mt-3 text-[13px] text-danger">{error} 결과 확인으로 다시 조회할 수 있어요.</p>}
  </section>;
}
