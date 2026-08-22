import { describe, expect, it } from "vitest";
import {
  agentCreationPayload,
  defaultControlValues,
  optionsForControl,
} from "./AgentSessionPanel";
import type { NativeCliProviderAvailability } from "./roomSocketTypes";

const provider: NativeCliProviderAvailability = {
  id: "codex",
  display_name: "Codex",
  provider_kind: "codex_live_session",
  runtime_kind: "live_cli",
  catalog_group: "subscription",
  workspace_required: true,
  connection_kind: "native_cli_bridge",
  default_model: "terra",
  interactive: true,
  startable: true,
  available: true,
  discovery_status: "ready",
  catalog_source: "discovered",
  login_available: true,
  login_label: "Login",
  login_flow: "browser_oauth",
  controls: [
    {
      key: "model",
      label: "Model",
      kind: "combobox",
      default_value: "terra",
      options: [
        {
          value: "terra",
          label: "Terra",
          metadata: { reasoning_efforts: ["medium"] },
        },
      ],
    },
    {
      key: "reasoning_effort",
      label: "Effort",
      kind: "select",
      default_value: "medium",
      options: [
        { value: "medium", label: "Medium" },
        { value: "high", label: "High" },
      ],
    },
  ],
};

describe("Agent Session creation contract", () => {
  it("uses generated catalog controls and retains the exact revision", () => {
    const defaults = defaultControlValues(provider);
    expect(defaults).toEqual({ model: "terra", reasoning_effort: "medium" });
    expect(optionsForControl(provider, "reasoning_effort", "terra").map(({ value }) => value))
      .toEqual(["medium"]);
    expect(agentCreationPayload(
      { status: "ready", catalog_revision: "catalog-exact", providers: [provider] },
      "codex",
      "  Terra  ",
      "  /workspace  ",
      defaults
    )).toEqual({
      provider_id: "codex",
      catalog_revision: "catalog-exact",
      display_name: "Terra",
      workspace: "  /workspace  ",
      start_now: false,
      model: "terra",
      reasoning_effort: "medium",
    });
  });
});
