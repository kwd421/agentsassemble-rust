import { describe, expect, it } from "vitest";
import { usageSummary } from "./AgentUsageLine";

describe("usageSummary", () => {
  it("reads like the DeepSeek harness footer", () => {
    expect(usageSummary({
      turns: 13, steps: 322, prompt_tokens: 49_000_000, cached_prompt_tokens: 48_853_000,
      completion_tokens: 1_000_000, last_prompt_tokens: 33_280, context_tokens: 128_000, model_ms: 4_132_000,
    })).toEqual(["13차례 · 322단계", "토큰 50M", "캐시 99.7%", "컨텍스트 26.0%", "응답 242 tok/s"]);
  });

  it("rounds before choosing a unit and never shows 100% early", () => {
    const line = (prompt: number, cached: number) => usageSummary({
      turns: 1, steps: 1, prompt_tokens: prompt, cached_prompt_tokens: cached,
      completion_tokens: 0, last_prompt_tokens: 0, context_tokens: 0, model_ms: 0,
    });
    expect(line(999_950, 0)[1]).toBe("토큰 1.0M");
    expect(line(9_999_999, 0)[1]).toBe("토큰 10M");
    expect(line(999, 0)[1]).toBe("토큰 999");
    expect(line(10_000, 9_996)[2]).toBe("캐시 99.9%");
    expect(line(10_000, 10_000)[2]).toBe("캐시 100%");
  });

  it("leaves speed out before any output and shows a dash without a window", () => {
    expect(usageSummary({
      turns: 1, steps: 1, prompt_tokens: 900, cached_prompt_tokens: 0,
      completion_tokens: 0, last_prompt_tokens: 900, context_tokens: 0, model_ms: 0,
    })).toEqual(["1차례 · 1단계", "토큰 900", "캐시 0.0%", "컨텍스트 —"]);
  });
});
