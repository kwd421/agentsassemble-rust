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
  return <section className="ops-inner rounded-xl p-5" aria-label="앱 점검">
    <div className="flex items-center justify-between gap-3">
      <h2 className="font-bold">앱 점검</h2>
      <button type="button" className="ops-button rounded-lg px-4" style={{ minHeight: 44 }} disabled={loading} onClick={() => {
        setResult(null); setError(false); setLoading(true); setRevision((value) => value + 1);
      }}>결과 새로고침</button>
    </div>
    <p className="mt-2 text-[12px] text-text-muted">마지막으로 저장된 점검 결과예요. 현재 앱 상태와 다를 수 있어요.</p>
    {loading && <p role="status">점검 결과를 읽고 있어요.</p>}
    {error && <p role="alert">저장된 점검 결과를 읽지 못했어요. 보고서를 확인하고 새로고침해 주세요.</p>}
    {result && <>
      <p className="mt-2 text-[12px] text-text-muted">{result.report ? `${new Date(result.report.completed_at).toLocaleString()} 완료` : "저장된 점검 결과가 없어요."}</p>
      <div className="mt-3 grid gap-2 sm:grid-cols-2">{result.checks.map((check) => <article key={check.id} className="ops-inner rounded-lg px-4 py-3">
        <strong className="text-[13px]">{checkLabels[check.id] ?? "앱 점검"}</strong>
        <p className="text-[12px] text-text-muted">{releaseHealthStatusLabel(latest.get(check.id)?.status ?? "not_run")}</p>
      </article>)}</div>
    </>}
  </section>;
}
