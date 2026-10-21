import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import AgentPersonaPicker from "./AgentPersonaPicker";
import { OwnerRouteHost } from "../../app/ownerRouteHost";

const personaApi = vi.hoisted(() => ({
  fetchPersonaAssets: vi.fn(),
  importPersonaAsset: vi.fn(),
  deletePersonaAsset: vi.fn(),
  fetchPersonaThumbnail: vi.fn(),
}));

vi.mock("../../api/personas", () => ({
  fetchPersonaAssets: personaApi.fetchPersonaAssets,
  importPersonaAsset: personaApi.importPersonaAsset,
  deletePersonaAsset: personaApi.deletePersonaAsset,
  fetchPersonaThumbnail: personaApi.fetchPersonaThumbnail,
}));

afterEach(cleanup);

beforeEach(() => {
  personaApi.fetchPersonaAssets.mockReset();
  personaApi.importPersonaAsset.mockReset();
  personaApi.deletePersonaAsset.mockReset();
  personaApi.fetchPersonaThumbnail.mockReset();
  personaApi.fetchPersonaThumbnail.mockResolvedValue(new Blob(["png"]));
  Object.defineProperty(URL, "createObjectURL", {
    configurable: true,
    value: vi
      .fn()
      .mockReturnValueOnce("blob:persona-thumbnail-1")
      .mockReturnValue("blob:persona-thumbnail-2"),
  });
  Object.defineProperty(URL, "revokeObjectURL", {
    configurable: true,
    value: vi.fn(),
  });
  personaApi.fetchPersonaAssets.mockResolvedValue([
    {
      id: "guide",
      display_name: "Night Guide",
      asset_kind: "card",
      source_kind: "ccv3",
      lorebook_count: 2,
      asset_count: 1,
      ignored_feature_count: 0,
      tag_count: 0,
      thumbnail_url: "/api/personas/guide/thumbnail",
    },
    {
      id: "weather-module",
      display_name: "Weather Module",
      asset_kind: "module",
      source_kind: "risu_module",
      lorebook_count: 4,
      asset_count: 0,
      ignored_feature_count: 3,
      tag_count: 0,
      thumbnail_url: "",
    },
  ]);
});

describe("AgentPersonaPicker", () => {
  it("owns authenticated thumbnail object URLs only while they are rendered", async () => {
    const onChange = vi.fn();
    const { container, unmount } = render(
      <AgentPersonaPicker
        value="guide"
        applied={{
          id: "guide",
          display_name: "Night Guide",
          asset_kind: "card",
          source_kind: "ccv3",
          lorebook_count: 2,
          asset_count: 1,
          ignored_feature_count: 0,
          tag_count: 0,
          thumbnail_url: "/api/personas/guide/thumbnail",
        }}
        onChange={onChange}
      />,
      { reactStrictMode: true }
    );

    await waitFor(() =>
      expect(personaApi.fetchPersonaThumbnail).toHaveBeenCalledWith(
        "guide",
        expect.any(AbortSignal),
        null
      )
    );
    await waitFor(() =>
      expect(container.querySelector("img")?.getAttribute("src")).toBe(
        "blob:persona-thumbnail-1"
      )
    );
    expect(URL.createObjectURL).toHaveBeenCalledOnce();

    unmount();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:persona-thumbnail-1");
  });

  it("makes an advertised thumbnail read failure visible", async () => {
    personaApi.fetchPersonaThumbnail.mockRejectedValueOnce(new Error("missing"));

    render(
      <AgentPersonaPicker
        value="guide"
        applied={{
          id: "guide",
          display_name: "Night Guide",
          asset_kind: "card",
          source_kind: "ccv3",
          lorebook_count: 2,
          asset_count: 1,
          ignored_feature_count: 0,
          tag_count: 0,
          thumbnail_url: "/api/personas/guide/thumbnail",
        }}
        onChange={vi.fn()}
      />
    );

    expect((await screen.findByRole("alert")).textContent).toContain(
      "봇카드 썸네일을 불러오지 못했어요."
    );
  });

  it("reads the library of the room's own server, or this computer's when asked", async () => {
    const view = render(<OwnerRouteHost.Provider value="server-b"><AgentPersonaPicker value="" onChange={vi.fn()} /></OwnerRouteHost.Provider>);
    await userEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(personaApi.fetchPersonaAssets).toHaveBeenLastCalledWith("server-b"));
    view.unmount();
    render(<OwnerRouteHost.Provider value="server-b"><AgentPersonaPicker value="" serverId={null} onChange={vi.fn()} /></OwnerRouteHost.Provider>);
    await userEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(personaApi.fetchPersonaAssets).toHaveBeenLastCalledWith(null));
  });

  it("distinguishes cards from modules and reports the applied selection", async () => {
    const onChange = vi.fn();
    render(
      <AgentPersonaPicker
        value="guide"
        applied={{
          id: "guide",
          display_name: "Night Guide",
          asset_kind: "card",
          source_kind: "ccv3",
          lorebook_count: 2,
          asset_count: 1,
          ignored_feature_count: 0,
          tag_count: 0,
          thumbnail_url: "/api/personas/guide/thumbnail",
        }}
        onChange={onChange}
      />
    );

    expect(personaApi.fetchPersonaAssets).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: /Night Guide/ })).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: /Night Guide/ }));
    await waitFor(() => expect(screen.getByText("Weather Module")).toBeTruthy());
    expect(screen.getByRole("radio", { name: /Night Guide/ }).getAttribute("aria-checked")).toBe("true");
    expect(screen.getByRole("radio", { name: /사용 안 함/ }).getAttribute("aria-checked")).toBe("false");
    expect(screen.getByText("적용됨")).toBeTruthy();
    expect(screen.getByText(/Risu 모듈 · 로어 4/)).toBeTruthy();

    await userEvent.click(screen.getByRole("radio", { name: /Weather Module/ }));
    expect(onChange).toHaveBeenCalledWith("weather-module");
  });

  it("takes keyboard focus into the menu and hands it back after a choice", async () => {
    const onChange = vi.fn();
    render(<AgentPersonaPicker value="guide" onChange={onChange} />);
    const trigger = screen.getByRole("button", { name: /선택|변경/ });
    await userEvent.click(trigger);
    await waitFor(() => expect(document.activeElement?.textContent).toContain("Night Guide"));
    await userEvent.keyboard("{ArrowDown}");
    expect(document.activeElement?.textContent).toContain("Weather Module");
    await userEvent.keyboard("{Enter}");
    expect(onChange).toHaveBeenCalledWith("weather-module");
    expect(document.activeElement).toBe(trigger);
  });

  it("deletes a card on the second press and keeps the applied one", async () => {
    const onChange = vi.fn();
    personaApi.deletePersonaAsset.mockResolvedValue(undefined);
    const applied = { id: "guide", display_name: "Night Guide", asset_kind: "card" as const, source_kind: "ccv3" as const,
      lorebook_count: 2, asset_count: 1, ignored_feature_count: 0, tag_count: 0, thumbnail_url: "" };
    render(<AgentPersonaPicker value="guide" applied={applied} onChange={onChange} />);
    await userEvent.click(screen.getByRole("button", { name: /선택|변경/ }));
    await screen.findByText("Weather Module");
    expect(screen.queryByRole("button", { name: "Night Guide 카드 지우기" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Weather Module 카드 지우기" }));
    expect(personaApi.deletePersonaAsset).not.toHaveBeenCalled();
    expect(screen.getByRole("status").textContent).toContain("한 번 더 누르면");
    // Pressing elsewhere in the menu disarms it; the next press only arms again.
    await userEvent.click(screen.getByRole("radio", { name: /Night Guide/ }));
    await userEvent.click(screen.getByRole("button", { name: /선택|변경/ }));
    await screen.findByText("Weather Module");
    await userEvent.click(screen.getByRole("button", { name: "Weather Module 카드 지우기" }));
    expect(personaApi.deletePersonaAsset).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "지우기" }));
    expect(personaApi.deletePersonaAsset).toHaveBeenCalledWith("weather-module", null);
    await waitFor(() => expect(screen.queryByText("Weather Module")).toBeNull());
    // Deleting another card never clears the agent's own selection.
    expect(onChange).not.toHaveBeenCalledWith("");
  });

  it("selects a newly imported supported file", async () => {
    const onChange = vi.fn();
    personaApi.importPersonaAsset.mockResolvedValue({
      id: "new-module",
      display_name: "New Module",
      asset_kind: "module",
      source_kind: "risu_module",
      lorebook_count: 1,
      asset_count: 0,
      ignored_feature_count: 0,
      tag_count: 0,
      thumbnail_url: "",
    });
    render(<AgentPersonaPicker value="" onChange={onChange} />);

    const input = screen.getByLabelText("파일 가져오기").querySelector("input") ||
      screen.getByLabelText("파일 가져오기");
    await userEvent.upload(input as HTMLInputElement, new File(["module"], "module.risum"));

    await waitFor(() => expect(onChange).toHaveBeenCalledWith("new-module"));
    expect(screen.getByText(/New Module 가져오기 완료/)).toBeTruthy();
  });

  it("replaces the rendered thumbnail after reimporting the same persona", async () => {
    personaApi.importPersonaAsset.mockResolvedValue({
      id: "guide",
      display_name: "Updated Guide",
      asset_kind: "card",
      source_kind: "ccv3",
      lorebook_count: 3,
      asset_count: 1,
      ignored_feature_count: 0,
      tag_count: 0,
      thumbnail_url: "/api/personas/guide/thumbnail",
    });
    const applied = {
      id: "guide",
      display_name: "Night Guide",
      asset_kind: "card" as const,
      source_kind: "ccv3" as const,
      lorebook_count: 2,
      asset_count: 1,
      ignored_feature_count: 0,
      tag_count: 0,
      thumbnail_url: "/api/personas/guide/thumbnail",
    };
    const { container } = render(
      <AgentPersonaPicker value="guide" applied={applied} onChange={vi.fn()} />
    );
    await waitFor(() =>
      expect(container.querySelector("img")?.getAttribute("src")).toBe(
        "blob:persona-thumbnail-1"
      )
    );

    const input = screen.getByLabelText("파일 가져오기").querySelector("input") ||
      screen.getByLabelText("파일 가져오기");
    await userEvent.upload(input as HTMLInputElement, new File(["card"], "guide.png"));

    await waitFor(() => expect(personaApi.fetchPersonaThumbnail).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(container.querySelector("img")?.getAttribute("src")).toBe(
        "blob:persona-thumbnail-2"
      )
    );
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:persona-thumbnail-1");
  });

  it("bounds a large library and finds a card through search", async () => {
    const onChange = vi.fn();
    personaApi.fetchPersonaAssets.mockResolvedValue(
      Array.from({ length: 100 }, (_, index) => ({
        id: `persona-${index}`,
        display_name: `Persona ${index}`,
        asset_kind: "card",
        source_kind: "ccv3",
        lorebook_count: 0,
        asset_count: 0,
        ignored_feature_count: 0,
        tag_count: 0,
        thumbnail_url: "",
      }))
    );
    render(<AgentPersonaPicker value="" onChange={onChange} />);

    await userEvent.click(screen.getByRole("button", { name: /사용 안 함/ }));
    await waitFor(() => expect(screen.getByText("Persona 0")).toBeTruthy());
    // Nothing is applied yet, so the list is the 8 shown cards; a "clear"
    // row would only repeat what the closed trigger already says.
    expect(screen.getAllByRole("radio")).toHaveLength(8);
    expect(screen.queryByText("Persona 99")).toBeNull();

    await userEvent.type(screen.getByPlaceholderText("봇카드 또는 모듈 검색"), "Persona 99");
    await userEvent.click(screen.getByRole("radio", { name: /Persona 99/ }));
    expect(onChange).toHaveBeenCalledWith("persona-99");
  });
});
