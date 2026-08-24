import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import ProviderLogo, { providerBrandKey } from "./ProviderLogo";
import { resolveProviderPresentation } from "./providerBranding";

afterEach(cleanup);

describe("ProviderLogo", () => {
  it("renders a distinct branded mark for every canonical agent provider", () => {
    const providers = [
      ["codex", "codex_live_session"],
      ["antigravity", "antigravity_live_session"],
      ["grok", "grok_live_session"],
      ["claude", "claude_code"],
      ["cursor", "cursor_live_session"],
      ["freebuff", "freebuff_live_session"],
      ["opencode", "opencode_server"],
      ["deepseek", "deepseek_api"],
      ["cerebras", "cerebras_api"],
      ["ollama", "ollama_api"],
      ["lmstudio", "lmstudio_api"],
      ["llmgateway", "llm_gateway_api"],
      ["openrouter", "openrouter_api"],
      ["tokenrouter", "tokenrouter_api"],
      ["custom_api", "custom_openai_api"],
      ["vercel", "vercel_ai_gateway"],
    ] as const;
    const { container } = render(
      <div>
        {providers.map(([id, kind]) => (
          <ProviderLogo key={id} providerId={id} providerKind={kind} />
        ))}
      </div>
    );

    providers.forEach(([id]) => {
      expect(container.querySelector(`[data-provider-brand="${id}"]`)).not.toBeNull();
    });
  });

  it("recognizes current provider identifiers and rejects unknown providers", () => {
    expect(providerBrandKey(undefined, "codex_live_session")).toBe("codex");
    expect(providerBrandKey(undefined, "local_cli")).toBeUndefined();
  });

  it("keeps provider product names, logo labels, and model names aligned", () => {
    const vercel = resolveProviderPresentation({
      providerId: "vercel",
      providerKind: "vercel_ai_gateway",
      providerDisplayName: "Vercel AI Gateway",
      modelLabel: "GPT-5.4 Mini",
    });
    expect(vercel.brand?.logoLabel).toBe("Vercel");
    expect(vercel.providerName).toBe("Vercel AI Gateway");
    expect(vercel.defaultAgentName).toBe("Vercel AI Gateway · GPT-5.4 Mini");

    expect(
      resolveProviderPresentation({
        providerId: "claude",
        providerDisplayName: "Claude",
        modelLabel: "Claude Sonnet 4.6",
      }).defaultAgentName
    ).toBe("Claude Sonnet 4.6");
  });
});
