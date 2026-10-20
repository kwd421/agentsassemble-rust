import RuntimeRestartPanel from "./components/RuntimeRestartPanel";
import { useEffect, useState } from "react";
import { Activity, X } from "../ui/icons";
import { fetchLocalResources, type LocalResourceStatus } from "../api/localResources";

const REFRESH_MS = 2000;

// `ownsLocalServer` is false when another account owns this computer's server:
// machine usage stays visible, but that owner's rooms and the restart control do not.
export default function AdminPanel({ onClose, ownsLocalServer = true }: { onClose: () => void; ownsLocalServer?: boolean }) {
  const [resources, setResources] = useState<LocalResourceStatus | null>(null);
  const [error, setError] = useState(false);
  // Like a task manager: keep reading while the page is open, and stop when it closes.
  useEffect(() => {
    let active = true;
    let timer = 0;
    const read = async () => {
      try {
        const value = await fetchLocalResources();
        if (!active) return;
        setResources(value); setError(false);
      } catch {
        if (!active) return;
        setResources(null); setError(true);
      }
      if (active) timer = window.setTimeout(() => { void read(); }, REFRESH_MS);
    };
    void read();
    return () => { active = false; window.clearTimeout(timer); };
  }, []);
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
  // Memory order holds still between readings; CPU order would reshuffle the list every time.
  const rows = [...groups.values()].sort((left, right) => (right.memory ?? -1) - (left.memory ?? -1));
  return (
    <section className="flex h-full min-h-0 w-full flex-col" aria-label="내 컴퓨터">
      <header className="dc-chat-head flex h-12 shrink-0 items-center gap-2 px-4">
        <Activity size={20} className="text-text-muted" aria-hidden />
        <h1 className="text-[16px] font-semibold text-text-primary">내 컴퓨터</h1>
        <div className="dc-head-actions ml-auto flex items-center gap-1.5">
          <button type="button" onClick={onClose} className="dc-head-icon" aria-label="내 컴퓨터 닫기" data-tip="닫기"><X size={20} /></button>
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto chat-scroll" data-shot-scroll>
        <div className="dc-computer-page">
          {!resources && !error && <p role="status" className="text-[14px] text-text-muted">사용량을 확인하고 있어요…</p>}
          {error && <p role="alert" className="text-[14px] text-danger">사용량을 불러오지 못했어요. 다시 확인하고 있어요.</p>}
          {resources && <section aria-label="앱과 관련 도구">
            <dl className="dc-computer-summary-line">
              <div><dt>메모리</dt><dd>{resources.available_memory_bytes === null ? "확인 불가" : `${memory(resources.available_memory_bytes)} 남음`} / <span>{memory(resources.total_memory_bytes)}</span></dd></div>
              <div><dt>CPU</dt><dd>{resources.cpu_count === null ? "확인 불가" : `${resources.cpu_count}코어`}</dd></div>
              <div><dt>도구</dt><dd>{resources.matching_process_count}개</dd></div>
            </dl>
            <table className="dc-computer-table" aria-label="프로세스 사용량">
              <thead><tr><th>이름</th><th data-tip="코어 하나를 다 쓰면 100%예요">CPU</th><th>메모리</th></tr></thead>
              <tbody>{rows.map(row => <tr key={row.name}>
                <td>{row.name}{row.count > 1 && <span className="text-text-muted"> × {row.count}</span>}</td>
                <td>{row.cpu === null ? "확인 중" : `${row.cpu.toFixed(1)}%`}</td><td>{memory(row.memory)}</td>
              </tr>)}</tbody>
            </table>
            <p className="mt-2 text-[12px] text-text-muted">다른 앱에서 실행한 같은 도구도 함께 보여요.</p>
          </section>}
          {resources && ownsLocalServer && <section className="dc-computer-card" aria-label="저장 공간">
            <div className="flex items-baseline justify-between gap-3">
              <h2 className="dc-computer-heading">저장 공간</h2>
              <span className="text-[12px] text-text-muted">올린 파일 기준</span>
            </div>
            <table className="dc-computer-table" aria-label="방별 저장 공간">
              <thead><tr><th>방</th><th>파일</th><th>용량</th></tr></thead>
              <tbody>
                {[...resources.attachment_storage.rooms].sort((left, right) => right.bytes - left.bytes).map(room => <tr key={room.room_id}>
                  <td>{room.label}</td><td>{room.files}개</td><td>{memory(room.bytes)}</td>
                </tr>)}
                {resources.attachment_storage.other.files > 0 && <tr>
                  <td className="text-text-muted">프로필 사진 등</td><td>{resources.attachment_storage.other.files}개</td><td>{memory(resources.attachment_storage.other.bytes)}</td>
                </tr>}
              </tbody>
              <tfoot><tr><td>합계</td><td>{resources.attachment_storage.total.files}개</td><td>{memory(resources.attachment_storage.total.bytes)}</td></tr></tfoot>
            </table>
          </section>}
          {ownsLocalServer && <RuntimeRestartPanel />}
        </div>
      </div>
    </section>
  );
}

function memory(bytes: number | null) {
  if (bytes === null) return "확인 불가";
  if (bytes === 0) return "0 MB";
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
