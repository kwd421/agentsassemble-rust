import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import AgentCreateModal from "./AgentCreateModal";
import { customApiProvider } from "./AgentCreateModal.testProviders";
import {
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

describe("AgentCreateModal Custom API", () => {
  it("requires and submits the caller-owned endpoint and model", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-custom"
        providers={[customApiProvider()]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    await userEvent.click(screen.getByRole("listitem", { name: "API" }));
    await userEvent.click(screen.getByRole("listitem", { name: "Custom API" }));
    expect(primaryActionButton().disabled).toBe(true);

    await userEvent.type(
      screen.getByLabelText("API 주소"),
      "https://api.example.com/v1/chat/completions"
    );
    expect(primaryActionButton().disabled).toBe(true);
    await userEvent.type(screen.getByLabelText("모델 ID"), "vendor-model");
    expect(primaryActionButton().disabled).toBe(false);

    await userEvent.type(screen.getByLabelText("API 키"), "secret-value");
    await userEvent.click(screen.getByRole("button", { name: "보안 저장" }));
    await waitFor(() =>
      expect(apiMocks.setProviderCredential).toHaveBeenCalledWith(
        "custom_api",
        "secret-value"
      )
    );
    await userEvent.click(primaryActionButton());

    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        providerId: "custom_api",
        providerEndpoint: "https://api.example.com/v1/chat/completions",
        modelId: "vendor-model",
        displayName: "Custom vendor-model",
        permissionMode: "meeting_read_only",
        maxOutputTokens: 4096,
      })
    );
  });
});
