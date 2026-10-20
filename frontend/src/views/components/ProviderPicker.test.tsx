import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import ProviderPicker from "./ProviderPicker";
import { codexProvider, deepSeekProvider, openCodeProvider } from "./AgentCreateModal.testProviders";

afterEach(cleanup);

const cursor = { ...codexProvider(), id: "cursor", display_name: "Cursor", provider_kind: "cursor_live_session" };

it("starts from one + card, searches, and leaves OAuth-only products out", () => {
  const onSelect = vi.fn();
  render(<ProviderPicker providers={[codexProvider(), deepSeekProvider(), openCodeProvider(), cursor]} selectedId="" onSelect={onSelect} />);
  expect(screen.queryAllByRole("listitem")).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "AI 고르기" }));
  expect(screen.queryByRole("listitem", { name: /Cursor/ })).toBeNull();
  fireEvent.change(screen.getByRole("searchbox", { name: "AI 검색" }), { target: { value: "deep" } });
  expect(screen.getAllByRole("listitem").map((item) => item.getAttribute("aria-label"))).toEqual(["DeepSeek"]);
  fireEvent.change(screen.getByRole("searchbox", { name: "AI 검색" }), { target: { value: "없는 AI" } });
  expect(screen.getByText("찾는 AI가 없어요.")).toBeTruthy();
  fireEvent.change(screen.getByRole("searchbox", { name: "AI 검색" }), { target: { value: "codex" } });
  fireEvent.click(screen.getByRole("listitem", { name: "ChatGPT Codex" }));
  expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: "codex" }));
});

it("shows the chosen AI with how it runs and lets the person change it", () => {
  render(<ProviderPicker providers={[codexProvider(), deepSeekProvider()]} selectedId="deepseek" onSelect={vi.fn()} />);
  const chosen = screen.getByRole("region", { name: "고른 AI" });
  expect(chosen.textContent).toContain("DeepSeek");
  expect(within(chosen).getByRole("radio", { name: "API" }).getAttribute("aria-checked")).toBe("true");
  fireEvent.click(within(chosen).getByRole("button", { name: "바꾸기" }));
  expect(screen.getByRole("searchbox", { name: "AI 검색" })).toBeTruthy();
  fireEvent.keyDown(screen.getByRole("searchbox", { name: "AI 검색" }), { key: "Escape" });
  expect(screen.getByRole("region", { name: "고른 AI" })).toBeTruthy();
});

it("narrows the list by how an AI runs and picks that way", () => {
  const onSelect = vi.fn();
  render(<ProviderPicker providers={[codexProvider(), deepSeekProvider(), openCodeProvider()]} selectedId="" onSelect={onSelect} />);
  fireEvent.click(screen.getByRole("button", { name: "AI 고르기" }));
  fireEvent.click(screen.getByRole("radio", { name: "API" }));
  expect(screen.getAllByRole("listitem").map((item) => item.getAttribute("aria-label"))).toEqual(["DeepSeek"]);
  fireEvent.click(screen.getByRole("radio", { name: "구독" }));
  expect(screen.getAllByRole("listitem").map((item) => item.getAttribute("aria-label"))).toEqual(["ChatGPT Codex", "OpenCode"]);
  fireEvent.click(screen.getByRole("radio", { name: "전체" }));
  expect(screen.getAllByRole("listitem")).toHaveLength(3);
});

it("keeps starred AIs on top across openings", () => {
  localStorage.clear();
  const view = render(<ProviderPicker providers={[codexProvider(), deepSeekProvider(), openCodeProvider()]} selectedId="" onSelect={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "AI 고르기" }));
  fireEvent.click(screen.getByRole("button", { name: "OpenCode 즐겨찾기" }));
  expect(screen.getByRole("button", { name: "OpenCode 즐겨찾기" }).getAttribute("aria-pressed")).toBe("true");
  expect(screen.getAllByRole("listitem")[0].getAttribute("aria-label")).toBe("OpenCode");
  view.unmount();
  render(<ProviderPicker providers={[codexProvider(), deepSeekProvider(), openCodeProvider()]} selectedId="" onSelect={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "AI 고르기" }));
  expect(screen.getAllByRole("listitem")[0].getAttribute("aria-label")).toBe("OpenCode");
  localStorage.clear();
});
