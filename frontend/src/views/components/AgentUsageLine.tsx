import { useEffect, useState } from "react";
import { fetchAgentUsage, type AgentUsageTotals } from "../../api/agentUsage";
import { useOwnerRouteHost } from "../../app/ownerRouteHost";

function compact(value: number): string {
  if (value < 1_000) return String(value);
  // The unit is chosen after rounding, so 999,950 reads "1.0M", not "1000.0K".
  const round = (scaled: number) => Number(scaled.toFixed(scaled >= 10 ? 0 : 1));
  const thousands = round(value / 1_000);
  if (thousands < 1_000) return `${thousands.toFixed(thousands >= 10 ? 0 : 1)}K`;
  const millions = round(value / 1_000_000);
  return `${millions.toFixed(millions >= 10 ? 0 : 1)}M`;
}

function percent(part: number, whole: number): string {
  if (whole <= 0) return "—";
  const value = (part / whole) * 100;
  // Only an exact match reads 100%; 99.96% stays below it.
  return part >= whole ? "100%" : `${Math.min(value, 99.9).toFixed(1)}%`;
}

/** The usage line under an API AI's settings, like the footer of DeepSeek's own harness. */
export function usageSummary(usage: AgentUsageTotals): string[] {
  const parts = [`${usage.turns}차례 · ${usage.steps}단계`, `토큰 ${compact(usage.prompt_tokens + usage.completion_tokens)}`,
    `캐시 ${percent(usage.cached_prompt_tokens, usage.prompt_tokens)}`, `컨텍스트 ${percent(usage.last_prompt_tokens, usage.context_tokens)}`];
  if (usage.model_ms > 0 && usage.completion_tokens > 0) {
    parts.push(`응답 ${Math.round(usage.completion_tokens / (usage.model_ms / 1000))} tok/s`);
  }
  return parts;
}

export default function AgentUsageLine({ roomId, sessionId, turnCount }: { roomId: string; sessionId: string; turnCount: number }) {
  const serverId = useOwnerRouteHost() ?? null;
  const [usage, setUsage] = useState<AgentUsageTotals | null>(null);
  useEffect(() => {
    let active = true;
    // A finished turn changes the turn count; read the totals again then.
    fetchAgentUsage(roomId, sessionId, serverId)
      .then((loaded) => { if (active) setUsage(loaded); })
      .catch(() => { if (active) setUsage(null); });
    return () => { active = false; };
  }, [roomId, sessionId, serverId, turnCount]);
  if (!usage || usage.turns === 0) return null;
  return (
    <p className="dc-agent-memory-status preserve-words" aria-label="모델 사용량"
      title="입력 토큰 중 캐시로 처리된 비율, 마지막 차례의 컨텍스트 사용률, 모델 대기 시간 기준 응답 속도예요.">
      {usageSummary(usage).join(" · ")}
    </p>
  );
}
