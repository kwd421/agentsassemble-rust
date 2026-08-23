import type { NativeCliProviderAvailability } from "../../roomSocketClient";

export function codexProvider(): NativeCliProviderAvailability {
  return {
    id: "codex",
    display_name: "Codex",
    provider_kind: "codex_cli",
    runtime_kind: "live_cli" as const,
    connection_kind: "native_cli_bridge" as const,
    executable: "codex",
    default_model: "gpt-5.6-luna",
    interactive: true as const,
    startable: true,
    available: true,
    controls: [
      {
        key: "model",
        label: "모델",
        kind: "combobox" as const,
        default_value: "gpt-5.6-luna",
        options: [{ value: "gpt-5.6-luna", label: "Luna" }],
      },
    ],
  };
}

export function claudeProvider(): NativeCliProviderAvailability {
  return {
    id: "claude",
    display_name: "Claude Code",
    provider_kind: "claude_code",
    runtime_kind: "live_cli" as const,
    connection_kind: "native_cli_bridge" as const,
    executable: "claude",
    default_model: "claude-haiku-4-5",
    interactive: true as const,
    startable: true,
    available: true,
    catalog_source: "static_manifest" as const,
    controls: [
      {
        key: "model",
        label: "모델",
        kind: "combobox" as const,
        default_value: "claude-haiku-4-5",
        options: [
          { value: "claude-haiku-4-5", label: "Claude Haiku 4.5" },
          { value: "claude-sonnet-4-6", label: "Claude Sonnet 4.6" },
        ],
      },
    ],
  };
}

export function deepSeekProvider(): NativeCliProviderAvailability {
  return {
    id: "deepseek",
    display_name: "DeepSeek",
    provider_kind: "deepseek_api",
    runtime_kind: "api",
    connection_kind: "native_cli_bridge",
    executable: "",
    default_model: "deepseek-chat",
    interactive: true,
    startable: true,
    available: true,
    catalog_group: "api",
    workspace_required: false,
    work_harness_available: true,
    controls: [
      {
        key: "max_output_tokens",
        label: "최대 응답 길이",
        kind: "select",
        default_value: "4096",
        options: [
          { value: "4096", label: "4,096 토큰" },
          { value: "8192", label: "8,192 토큰" },
        ],
      },
      workPermissionControl(),
    ],
  };
}

export function workPermissionControl() {
  return {
    key: "permission_mode",
    label: "권한",
    kind: "select" as const,
    default_value: "meeting_read_only",
    options: [
      { value: "meeting_read_only", label: "읽기 전용" },
      { value: "workspace_write", label: "작업 폴더 쓰기" },
    ],
  };
}

export function cerebrasProvider(): NativeCliProviderAvailability {
  return {
    ...deepSeekProvider(),
    id: "cerebras",
    display_name: "Cerebras",
    provider_kind: "cerebras_api",
    default_model: "gpt-oss-120b",
  };
}

export function ollamaProvider(): NativeCliProviderAvailability {
  return {
    ...deepSeekProvider(),
    id: "ollama",
    display_name: "Ollama",
    provider_kind: "ollama_api",
    catalog_group: "subscription",
    workspace_required: false,
    default_model: "nemotron-3-super:cloud",
    controls: [
      {
        key: "model",
        label: "모델",
        kind: "combobox",
        default_value: "nemotron-3-super:cloud",
        options: [
          {
            value: "nemotron-3-super:cloud",
            label: "Nemotron 3 Super",
            metadata: {
              catalog_group: "subscription",
              execution_location: "cloud",
              pricing: "free_tier",
            },
          },
          {
            value: "gemma4:12b",
            label: "Gemma 4 12B",
            metadata: {
              catalog_group: "local",
              execution_location: "local",
            },
          },
        ],
      },
      workPermissionControl(),
    ],
  };
}

export function openCodeProvider(): NativeCliProviderAvailability {
  return {
    ...deepSeekProvider(),
    id: "opencode",
    display_name: "OpenCode",
    provider_kind: "opencode_server",
    runtime_kind: "opencode",
    catalog_group: "subscription",
    workspace_required: true,
    work_harness_available: false,
    executable: "opencode",
    default_model: "opencode-go/glm-5.2",
    controls: [
      {
        key: "model",
        label: "모델",
        kind: "combobox",
        default_value: "opencode-go/glm-5.2",
        options: [
          {
            value: "opencode/deepseek-v4-flash-free",
            label: "DeepSeek V4 Flash",
            metadata: { group: "Zen", pricing: "free" },
          },
          {
            value: "opencode/big-pickle",
            label: "Big Pickle",
            metadata: { group: "Zen", pricing: "free" },
          },
          {
            value: "opencode-go/glm-5.2",
            label: "GLM 5.2",
            metadata: { group: "Go" },
          },
          {
            value: "opencode-go/kimi-k3",
            label: "Kimi K3",
            metadata: { group: "Go" },
          },
        ],
      },
    ],
  };
}

export function lmStudioProvider(): NativeCliProviderAvailability {
  return {
    ...deepSeekProvider(),
    id: "lmstudio",
    display_name: "LM Studio",
    provider_kind: "lmstudio_api",
    catalog_group: "local",
    workspace_required: false,
    default_model: "gemma-4-e4b-it",
    controls: [
      {
        key: "model",
        label: "모델",
        kind: "combobox",
        default_value: "gemma-4-e4b-it",
        options: [{ value: "gemma-4-e4b-it", label: "Gemma 4 E4B IT" }],
      },
      workPermissionControl(),
    ],
  };
}

export function codexProviderWithRelations(): NativeCliProviderAvailability {
  return {
    ...codexProvider(),
    default_model: "model-low",
    controls: [
      {
        key: "model",
        label: "모델",
        kind: "combobox" as const,
        default_value: "model-low",
        options: [
          {
            value: "model-low",
            label: "Low model",
            metadata: { reasoning_efforts: ["low"], service_tiers: ["priority"] },
          },
          {
            value: "model-high",
            label: "High model",
            metadata: { reasoning_efforts: ["high"], service_tiers: [] },
          },
          {
            value: "model-variable",
            label: "Variable model",
            metadata: {
              reasoning_efforts: ["low", "high"],
              service_tiers: ["fast"],
              runtime_variants: [
                { reasoning_effort: "low", service_tier: "default" },
                { reasoning_effort: "high", service_tier: "default" },
                { reasoning_effort: "high", service_tier: "fast" },
              ],
            },
          },
        ],
      },
      {
        key: "reasoning_effort",
        label: "추론 강도",
        kind: "select" as const,
        default_value: "low",
        options: [
          { value: "low", label: "low" },
          { value: "high", label: "high" },
        ],
      },
      {
        key: "service_tier",
        label: "응답 속도",
        kind: "select" as const,
        default_value: "default",
        options: [
          { value: "default", label: "기본" },
          { value: "priority", label: "Fast" },
          { value: "fast", label: "Fast" },
        ],
      },
    ],
  };
}
