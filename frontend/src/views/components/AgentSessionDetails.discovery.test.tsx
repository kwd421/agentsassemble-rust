import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { agentSessionFixture } from "../../test/agentSession";
import { geminiProvider } from "./AgentCreateModal.testProviders";
import AgentSessionDetails from "./AgentSessionDetails";

const discovery = vi.hoisted(() => ({ refresh: vi.fn() }));

vi.mock("../../api/providerOperations", () => ({
  refreshLocalProviderCatalog: discovery.refresh,
}));
vi.mock("../../lib/desktopBridge", async (original) => ({
  ...(await original<typeof import("../../lib/desktopBridge")>()),
  isDesktopWebview: () => true,
}));
vi.mock("../../api/personas", () => ({
  fetchPersonaAssets: vi.fn().mockResolvedValue([]),
  importPersonaAsset: vi.fn(),
}));

afterEach(() => {
  cleanup();
  discovery.refresh.mockReset();
});

it("checks OpenCode when opening an agent that runs through it, so its settings can be saved", () => {
  discovery.refresh.mockResolvedValue({});
  render(
    <AgentSessionDetails
      session={agentSessionFixture({ provider_kind: "gemini_api", execution_harness: "opencode", enabled: false })}
      provider={geminiProvider()}
      onConfigure={vi.fn()}
      localProviderActions
    />
  );
  expect(discovery.refresh.mock.calls.map(([id]) => id)).toEqual(["opencode"]);
});

it("leaves OpenCode alone for an agent on the built-in harness", () => {
  render(
    <AgentSessionDetails
      session={agentSessionFixture({ provider_kind: "gemini_api", execution_harness: "builtin", enabled: false })}
      provider={geminiProvider()}
      onConfigure={vi.fn()}
      localProviderActions
    />
  );
  expect(discovery.refresh).not.toHaveBeenCalled();
});
