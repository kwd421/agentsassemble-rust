import { useEffect, useState } from "react";
import { readReleaseHealth } from "../../api/releaseHealth";
import { releaseHealthStatusLabel } from "../../lib/releaseHealthLabels";

const checkLabels: Record<string, string> = {
  frontend_build: "화면 준비", frontend_tests: "화면 동작",
  rust_check: "앱 서버 준비", rust_tests: "앱 서버 동작",
  architecture: "앱 구성", git_diff: "변경 내용",
};

export default function ReleaseHealthPanel() {
  const [result, setResult] = useState<Awaited<ReturnType<typeof readReleaseHealth>> | null>(null);
  const [error, setError] = useState(false);
  const [loading, setLoading] = useState(true);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let active = true;
    void readReleaseHealth().then((value) => {
      if (active) setResult(value);
    }).catch(() => {
      if (active) setError(true);
    }).finally(() => {
      if (active) setLoading(false);
    });
    return () => { active = false; };
  }, [revision]);
  const latest = new Map(result?.report?.results.map((entry) => [entry.check_id, entry]));
  return <details className="dc-computer-card" aria-label="앱 점검">
    <summary className="dc-computer-summary">
      <span className="dc-computer-heading">앱 점검</span>
      <span className="text-[12px] text-text-muted">{loading ? "읽는 중…" : error ? "읽지 못했어요" : result?.report
        ? `${new Date(result.report.completed_at).toLocaleString("ko-KR", { month: "long", day: "numeric", hour: "numeric", minute: "2-digit" })} 점검`
        : "저장된 결과 없음"}</span>
    </summary>
    {error && <p role="alert" className="mt-3 text-[13px] text-danger">저장된 점검 결과를 읽지 못했어요.</p>}
    {result && <ul className="dc-computer-checks">{result.checks.map((check) => <li key={check.id}>
      <span>{checkLabels[check.id] ?? "앱 점검"}</span>
      <span className="text-text-muted">{releaseHealthStatusLabel(latest.get(check.id)?.status ?? "not_run")}</span>
    </li>)}</ul>}
    <button type="button" className="dc-quiet-button mt-3" disabled={loading} onClick={() => {
      setResult(null); setError(false); setLoading(true); setRevision((value) => value + 1);
    }}>결과 새로고침</button>
  </details>;
}
