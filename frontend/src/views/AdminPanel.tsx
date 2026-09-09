import ReleaseHealthPanel from "./components/ReleaseHealthPanel";
import { useEffect, useState } from "react";
import { Activity, RefreshCw, X } from "lucide-react";
import { fetchLocalResources, type LocalResourceStatus } from "../api/localResources";

export default function AdminPanel({ onClose }: { onClose: () => void }) {
  const [resources, setResources] = useState<LocalResourceStatus | null>(null);
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(true);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true;
    void fetchLocalResources().then((value) => {
      if (active) setResources(value);
    }).catch(() => {
      if (active) setError(true);
    }).finally(() => {
      if (active) setLoading(false);
    });
    return () => { active = false; };
  }, [revision]);
  const refresh = () => {
    setResources(null); setError(false); setLoading(true); setRevision((value) => value + 1);
  };
  return (
    <section className="ops-panel mx-auto flex h-full min-h-0 w-full max-w-5xl flex-col overflow-hidden" aria-label="서버 상태">
      <header className="flex shrink-0 items-center justify-between gap-3 border-b border-accent/14 px-5 py-4" style={{ padding: 24 }}>
        <h1 className="flex items-center gap-3 text-[20px] font-black"><Activity size={20} />서버 상태</h1>
        <div className="flex gap-2">
          <button type="button" onClick={refresh} disabled={loading} className="ops-button grid h-10 w-10 place-items-center rounded-lg" style={{ width: 44, height: 44 }} aria-label="상태 새로고침"><RefreshCw size={18} /></button>
          <button type="button" onClick={onClose} className="ops-button grid h-10 w-10 place-items-center rounded-lg" style={{ width: 44, height: 44 }} aria-label="서버 상태 닫기"><X size={18} /></button>
        </div>
      </header>
      <div className="flex-1 space-y-5 overflow-y-auto px-5 py-5 chat-scroll" style={{ padding: 24 }}>
        {loading && <p role="status">자원 사용량을 확인하고 있어요.</p>}
        {error && <p role="alert">자원 정보를 불러오지 못했어요. 새로고침해 주세요.</p>}
        {resources && <>
          <p className="text-[12px] text-text-muted">{new Date(resources.observed_at).toLocaleString()} 기준 · 수동 새로고침</p>
          <div className="grid gap-3 text-[13px] sm:grid-cols-2 lg:grid-cols-4">
            <Metric label="CPU 코어" value={resources.cpu_count?.toString() ?? "확인 불가"} />
            <Metric label="전체 메모리" value={memory(resources.total_memory_bytes)} />
            <Metric label="사용 가능한 메모리" value={memory(resources.available_memory_bytes)} />
            <Metric label="부하 평균 (1·5·15분)" value={resources.load_average?.map((value) => value.toFixed(2)).join(" · ") ?? "확인 불가"} />
          </div>
          <section className="flex flex-col gap-3" aria-label="관련 프로세스">
            <h2 className="mb-4 font-bold">앱과 관련 도구 · {resources.matching_process_count}개</h2>
            <p className="text-[12px] text-text-muted">최대 30개를 표시해요. 다른 앱에서 실행한 같은 도구도 포함돼요.</p>
            <p className="text-[12px] text-text-muted">{resources.cpu_sample_seconds === null
              ? "CPU는 비교할 표본이 필요해요. 잠시 후 새로고침해 주세요."
              : `CPU는 최근 ${resources.cpu_sample_seconds.toFixed(1)}초 평균이며 코어 하나가 100%예요.`}</p>
            {resources.processes.map((process) => <article key={process.pid} className="ops-inner rounded-lg p-4 text-[13px]">
              <strong>{process.label}</strong><span className="text-text-muted"> · PID {process.pid}</span>
              <p>CPU {process.cpu_percent === null ? "측정 전 또는 확인 불가" : `${process.cpu_percent.toFixed(1)}%`} · 메모리 {memory(process.memory_bytes)}</p>
            </article>)}
          </section>
        </>}
        <ReleaseHealthPanel />
      </div>
    </section>
  );
}

function memory(bytes: number | null) {
  return bytes === null ? "확인 불가" : `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}
function Metric({ label, value }: { label: string; value: string }) {
  return <div className="ops-inner rounded-lg p-4"><span className="text-text-muted">{label}</span> <strong>{value}</strong></div>;
}
