import RuntimeRestartPanel from "./components/RuntimeRestartPanel";
import ReleaseHealthPanel from "./components/ReleaseHealthPanel";
import { useEffect, useState, type ReactNode } from "react";
import { Activity, RefreshCw, X } from "lucide-react";
import { fetchLocalResources, type LocalResourceStatus } from "../api/localResources";

export default function AdminPanel({ onClose }: { onClose: () => void }) {
  const [resources, setResources] = useState<LocalResourceStatus | null>(null);
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(true);
  const [expanded, setExpanded] = useState(false);
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
  const groups = new Map<string, { name: string; count: number; cpu: number | null; memory: number | null }>();
  for (const process of resources?.processes || []) {
    const name = toolName(process.label);
    const group = groups.get(name);
    if (!group) groups.set(name, { name, count: 1, cpu: process.cpu_percent, memory: process.memory_bytes });
    else {
      group.count += 1;
      group.cpu = group.cpu === null || process.cpu_percent === null ? null : group.cpu + process.cpu_percent;
      group.memory = group.memory === null || process.memory_bytes === null ? null : group.memory + process.memory_bytes;
    }
  }
  const rows = [...groups.values()];
  const used = resources && resources.total_memory_bytes !== null && resources.available_memory_bytes !== null
    ? Math.min(1, Math.max(0, 1 - resources.available_memory_bytes / resources.total_memory_bytes)) : null;
  return (
    <section className="flex h-full min-h-0 w-full flex-col" aria-label="내 컴퓨터">
      <header className="dc-chat-head flex h-12 shrink-0 items-center gap-2 px-4">
        <Activity size={20} className="text-text-muted" aria-hidden />
        <h1 className="text-[16px] font-semibold text-text-primary">내 컴퓨터</h1>
        <div className="dc-head-actions ml-auto flex items-center gap-1.5">
          <button type="button" onClick={refresh} disabled={loading} className="dc-head-icon" aria-label="상태 새로고침" title="새로고침"><RefreshCw size={20} /></button>
          <button type="button" onClick={onClose} className="dc-head-icon" aria-label="내 컴퓨터 닫기" title="닫기"><X size={20} /></button>
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto chat-scroll" data-shot-scroll>
        <div className="dc-computer-page">
          {loading && <p role="status" className="text-[14px] text-text-muted">사용량을 확인하고 있어요…</p>}
          {error && <p role="alert" className="text-[14px] text-danger">사용량을 불러오지 못했어요. 새로고침해 주세요.</p>}
          {resources && <>
            <div className="grid gap-3 sm:grid-cols-3">
              <Metric label="메모리" value={resources.available_memory_bytes === null ? "확인 불가" : `${memory(resources.available_memory_bytes)} 남음`}
                detail={<>전체 <span>{memory(resources.total_memory_bytes)}</span></>} meter={used} />
              <Metric label="CPU 코어" value={resources.cpu_count === null ? "확인 불가" : `${resources.cpu_count}개`} />
              <Metric label="실행 중인 도구" value={`${resources.matching_process_count}개`} />
            </div>
            <section aria-label="앱과 관련 도구">
              <div className="flex items-baseline justify-between gap-3">
                <h2 className="dc-computer-heading">실행 중인 도구</h2>
                <time className="text-[12px] text-text-muted" dateTime={resources.observed_at}>
                  {new Date(resources.observed_at).toLocaleTimeString("ko-KR", { hour: "numeric", minute: "2-digit" })} 기준
                </time>
              </div>
              <table className="dc-computer-table" aria-label="프로세스 사용량">
                <thead><tr><th>이름</th><th title="코어 하나를 다 쓰면 100%예요">CPU</th><th>메모리</th></tr></thead>
                <tbody>{(expanded ? rows : rows.slice(0, 5)).map(row => <tr key={row.name}>
                  <td>{row.name}{row.count > 1 && <span className="text-text-muted"> × {row.count}</span>}</td>
                  <td>{row.cpu === null ? "확인 중" : `${row.cpu.toFixed(1)}%`}</td><td>{memory(row.memory)}</td>
                </tr>)}</tbody>
              </table>
              <div className="mt-2 flex items-center justify-between gap-3">
                <p className="text-[12px] text-text-muted">다른 앱에서 실행한 같은 도구도 함께 보여요.</p>
                {rows.length > 5 && <button type="button" className="dc-quiet-button" aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>{expanded ? "접기" : "더 보기"}</button>}
              </div>
            </section>
          </>}
          <RuntimeRestartPanel />
          <ReleaseHealthPanel />
        </div>
      </div>
    </section>
  );
}

function memory(bytes: number | null) {
  if (bytes === null) return "확인 불가";
  return bytes >= 1_000_000_000 ? `${(bytes / 1_000_000_000).toFixed(1)} GB` : `${(bytes / 1_000_000).toFixed(1)} MB`;
}
function toolName(label: string) {
  const names: Record<string, string> = {
    AgentsAssemble: "앱 서버", "runtime-child": "앱 관련 도구",
    claude: "Claude Code", "claude-code": "Claude Code", codex: "Codex",
    node: "Node", npm: "npm", python: "Python", python3: "Python", vite: "Vite",
    antigravity: "Antigravity", "antigravity-cli": "Antigravity",
    cursor: "Cursor", "cursor-agent": "Cursor", deepseek: "DeepSeek",
    grok: "Grok", "grok-cli": "Grok", hermes: "Hermes", kiro: "Kiro",
  };
  return names[label] ?? "앱 관련 도구";
}
function Metric({ label, value, detail, meter }: { label: string; value: string; detail?: ReactNode; meter?: number | null }) {
  return <div className="dc-computer-metric">
    <span className="text-[12px] text-text-muted">{label}</span>
    <strong>{value}</strong>
    {detail && <span className="text-[12px] text-text-muted">{detail}</span>}
    {meter !== undefined && meter !== null && <span className="dc-computer-meter" aria-hidden><span style={{ width: `${Math.round(meter * 100)}%` }} /></span>}
  </div>;
}
