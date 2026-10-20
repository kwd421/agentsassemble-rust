import { afterEach, describe, expect, it, vi } from "vitest";

const runtime = vi.hoisted(() => ({ fetch: vi.fn() }));
vi.mock("../lib/desktopBridge", () => ({ fetchDesktopOperatorRuntime: (...args: unknown[]) => runtime.fetch(...args) }));
vi.mock("../lib/ownerSessionTransport", () => ({ fetchOwnerSession: vi.fn() }));

import { saveFriend } from "./friends";

const details = {
  display_name: "deepseek", handle: "", participant_type: "api" as const, provider_kind: "deepseek", connection_kind: "",
  agent_id: "", source_agent_id: "", last_meeting_id: "", status: "offline", source: "manual", last_seen_at: null,
};

describe("saveFriend", () => {
  afterEach(() => runtime.fetch.mockReset());

  it.each([
    ["", undefined],
    ["deepseek-chat", "deepseek-chat"],
  ])("sends the model only when one is set (%s)", async (model, sent) => {
    runtime.fetch.mockResolvedValue(new Response(JSON.stringify({}), { status: 200 }));
    await saveFriend({ friend_id: "f", expected_revision: 0, details: { ...details, model } });
    const body = JSON.parse(String(runtime.fetch.mock.calls[0][1].body));
    expect(body.details.model).toBe(sent);
    expect("model" in body.details).toBe(sent !== undefined);
  });
});
