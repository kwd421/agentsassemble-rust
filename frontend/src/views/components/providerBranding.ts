import claudeLogo from "../../assets/provider-logos/claude.svg";
import cerebrasLogo from "../../assets/provider-logos/cerebras.svg";
import cursorLogo from "../../assets/provider-logos/cursor.png";
import deepSeekLogo from "../../assets/provider-logos/deepseek.png";
import freebuffLogo from "../../assets/provider-logos/freebuff.png";
import geminiLogo from "../../assets/provider-logos/gemini.webp";
import grokLogo from "../../assets/provider-logos/grok.png";
import lmStudioLogo from "../../assets/provider-logos/lmstudio.png";
import llmGatewayLogo from "../../assets/provider-logos/llmgateway.svg";
import ollamaLogo from "../../assets/provider-logos/ollama.png";
import openAILogo from "../../assets/provider-logos/openai.svg";
import openCodeLogo from "../../assets/provider-logos/opencode.png";
import openRouterLogo from "../../assets/provider-logos/openrouter.svg";
import tokenRouterLogo from "../../assets/provider-logos/tokenrouter.png";
import vercelLogo from "../../assets/provider-logos/vercel.svg";

export type ProviderBrandKey =
  | "codex"
  | "antigravity"
  | "grok"
  | "claude"
  | "cursor"
  | "freebuff"
  | "opencode"
  | "deepseek"
  | "cerebras"
  | "ollama"
  | "lmstudio"
  | "llmgateway"
  | "openrouter"
  | "tokenrouter"
  | "custom_api"
  | "vercel";

export type ProviderBrand = {
  productName: string;
  logoLabel: string;
  logo?: string;
  background: string;
  scale: string;
};

export type ProviderPresentation = {
  brandKey?: ProviderBrandKey;
  brand?: ProviderBrand;
  providerName: string;
  defaultAgentName: string;
};

const PROVIDER_ALIASES: Record<string, ProviderBrandKey> = {
  codex: "codex",
  codex_live_session: "codex",
  antigravity: "antigravity",
  antigravity_live_session: "antigravity",
  grok: "grok",
  grok_live_session: "grok",
  claude: "claude",
  claude_code: "claude",
  cursor: "cursor",
  cursor_live_session: "cursor",
  freebuff: "freebuff",
  freebuff_live_session: "freebuff",
  opencode: "opencode",
  opencode_server: "opencode",
  deepseek: "deepseek",
  deepseek_api: "deepseek",
  cerebras: "cerebras",
  cerebras_api: "cerebras",
  ollama: "ollama",
  ollama_api: "ollama",
  lmstudio: "lmstudio",
  lmstudio_api: "lmstudio",
  llmgateway: "llmgateway",
  llm_gateway_api: "llmgateway",
  openrouter: "openrouter",
  openrouter_api: "openrouter",
  tokenrouter: "tokenrouter",
  tokenrouter_api: "tokenrouter",
  custom_api: "custom_api",
  custom_openai_api: "custom_api",
  vercel: "vercel",
  vercel_ai_gateway: "vercel",
};

// Source assets have very different built-in whitespace. These scales normalize
// the contrasting brand mark to roughly 70% of the circular badge, not the raw
// image canvas.
export const PROVIDER_BRANDS: Record<ProviderBrandKey, ProviderBrand> = {
  codex: {
    productName: "Codex",
    logoLabel: "OpenAI",
    logo: openAILogo,
    background: "#000000",
    scale: "141%",
  },
  antigravity: {
    productName: "Antigravity",
    logoLabel: "Google Gemini",
    logo: geminiLogo,
    background: "#ffffff",
    scale: "79%",
  },
  grok: {
    productName: "Grok",
    logoLabel: "Grok",
    logo: grokLogo,
    background: "#000000",
    scale: "133%",
  },
  claude: {
    productName: "Claude",
    logoLabel: "Claude",
    logo: claudeLogo,
    background: "#d97757",
    scale: "96%",
  },
  cursor: {
    productName: "Cursor",
    logoLabel: "Cursor",
    logo: cursorLogo,
    background: "#0f0e0b",
    scale: "103%",
  },
  freebuff: {
    productName: "Freebuff",
    logoLabel: "Freebuff",
    logo: freebuffLogo,
    background: "#000000",
    scale: "100%",
  },
  opencode: {
    productName: "OpenCode",
    logoLabel: "OpenCode",
    logo: openCodeLogo,
    background: "#171515",
    scale: "111%",
  },
  deepseek: {
    productName: "DeepSeek",
    logoLabel: "DeepSeek",
    logo: deepSeekLogo,
    background: "#ffffff",
    scale: "70%",
  },
  cerebras: {
    productName: "Cerebras",
    logoLabel: "Cerebras",
    logo: cerebrasLogo,
    background: "#ffffff",
    scale: "70%",
  },
  ollama: {
    productName: "Ollama",
    logoLabel: "Ollama",
    logo: ollamaLogo,
    background: "#ffffff",
    scale: "87%",
  },
  lmstudio: {
    productName: "LM Studio",
    logoLabel: "LM Studio",
    logo: lmStudioLogo,
    background: "#5d45dd",
    scale: "89%",
  },
  llmgateway: {
    productName: "LLM Gateway",
    logoLabel: "LLM Gateway",
    logo: llmGatewayLogo,
    background: "#151515",
    scale: "76%",
  },
  openrouter: {
    productName: "OpenRouter",
    logoLabel: "OpenRouter",
    logo: openRouterLogo,
    background: "#ffffff",
    scale: "82%",
  },
  tokenrouter: {
    productName: "TokenRouter",
    logoLabel: "TokenRouter",
    logo: tokenRouterLogo,
    background: "#0b1020",
    scale: "74%",
  },
  custom_api: {
    productName: "Custom API",
    logoLabel: "Custom API",
    background: "#475569",
    scale: "58%",
  },
  vercel: {
    productName: "Vercel AI Gateway",
    logoLabel: "Vercel",
    logo: vercelLogo,
    background: "#000000",
    scale: "74%",
  },
};

function normalizeProviderIdentifier(value?: string) {
  return String(value || "").trim().toLowerCase().replaceAll("-", "_");
}

export function providerBrandKey(
  providerId?: string,
  providerKind?: string
): ProviderBrandKey | undefined {
  return (
    PROVIDER_ALIASES[normalizeProviderIdentifier(providerId)] ||
    PROVIDER_ALIASES[normalizeProviderIdentifier(providerKind)]
  );
}

function modelAlreadyNamesProvider(modelName: string, providerName: string): boolean {
  const providerToken = providerName.toLocaleLowerCase();
  const modelToken = modelName.toLocaleLowerCase();
  return (
    modelToken === providerToken ||
    [" ", "-", "·", "/", ":"].some((separator) =>
      modelToken.startsWith(`${providerToken}${separator}`)
    )
  );
}

export function resolveProviderPresentation({
  providerId,
  providerKind,
  providerDisplayName,
  modelLabel,
}: {
  providerId?: string;
  providerKind?: string;
  providerDisplayName?: string;
  modelLabel?: string;
}): ProviderPresentation {
  const brandKey = providerBrandKey(providerId, providerKind);
  const brand = brandKey ? PROVIDER_BRANDS[brandKey] : undefined;
  const providerName = String(providerDisplayName || "").trim() || brand?.productName || "";
  const modelName = String(modelLabel || "").trim();
  const defaultAgentName =
    !providerName || !modelName || modelAlreadyNamesProvider(modelName, providerName)
      ? modelName || providerName
      : `${providerName} · ${modelName}`;
  return { brandKey, brand, providerName, defaultAgentName };
}
