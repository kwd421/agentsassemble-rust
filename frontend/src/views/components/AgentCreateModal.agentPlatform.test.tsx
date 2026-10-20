import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import AgentCreateModal from "./AgentCreateModal";
import { agentPlatformProvider } from "./AgentCreateModal.testProviders";
import {
  pickAi,
  primaryActionButton,
  resetAgentCreateApiMocks,
} from "./AgentCreateModal.testUi";

const apiMocks = vi.hoisted(() => ({
  chooseLocalWorkspace: vi.fn(),
  deleteProviderCredential: vi.fn(),
  fetchProviderCredentialStatus: vi.fn(),
  setProviderCredential: vi.fn(),
}));

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  chooseLocalWorkspace: apiMocks.chooseLocalWorkspace,
  deleteProviderCredential: apiMocks.deleteProviderCredential,
  fetchProviderCredentialStatus: apiMocks.fetchProviderCredentialStatus,
  setProviderCredential: apiMocks.setProviderCredential,
}));

afterEach(cleanup);

beforeEach(() => {
  resetAgentCreateApiMocks(apiMocks);
});

describe("AgentCreateModal Agent Platform", () => {
  it("asks for the API key, project and region and submits the caller settings", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-agent-platform"
        providers={[agentPlatformProvider()]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    pickAi("Agent Platform");
    expect(primaryActionButton().disabled).toBe(true);
    expect(screen.getByLabelText("API 키")).toBeTruthy();
    const region = screen.getByLabelText("지역") as HTMLInputElement;
    expect(region.value).toBe("global");

    await userEvent.type(screen.getByLabelText("프로젝트 ID"), "my-project-123");
    expect(primaryActionButton().disabled).toBe(false);

    await userEvent.clear(region);
    await userEvent.type(region, "us-central1");

    await userEvent.type(screen.getByLabelText("API 키"), "secret-value");
    await userEvent.click(screen.getByRole("button", { name: "저장" }));
    await waitFor(() =>
      expect(apiMocks.setProviderCredential).toHaveBeenCalledWith(
        "agent_platform",
        "secret-value"
      )
    );
    await userEvent.click(primaryActionButton());

    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        providerId: "agent_platform",
        providerEndpoint: "projects/my-project-123/locations/us-central1",
        modelId: "google/gemini-2.5-flash-lite",
        permissionMode: "meeting_read_only",
        maxOutputTokens: 4096,
      })
    );
  });
});
