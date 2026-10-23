import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import ProviderControlSelect from "./ProviderControlSelect";

afterEach(cleanup);

function fastClick(element: HTMLElement) {
  fireEvent.pointerDown(element);
  fireEvent.pointerUp(element);
  fireEvent.click(element);
}

describe("ProviderControlSelect", () => {
  it("marks Fast and Priority with lightning, Flex with a coin stack, and the default tier with no icon", async () => {
    const options = [
      { value: "default", label: "기본" },
      { value: "fast", label: "Fast" },
      { value: "priority", label: "Priority" },
      { value: "flex", label: "Flex" },
    ];
    const view = render(
      <ProviderControlSelect
        label="응답 속도"
        options={options}
        value="default"
        showSpeedIcon
        onChange={vi.fn()}
      />
    );
    const trigger = screen.getByRole("combobox", { name: "응답 속도" });
    expect(trigger.querySelector('[data-icon="zap"]')).toBeNull();
    expect(trigger.querySelector('[data-icon="coins"]')).toBeNull();

    view.rerender(
      <ProviderControlSelect
        label="응답 속도"
        options={options}
        value="priority"
        showSpeedIcon
        onChange={vi.fn()}
      />
    );
    expect(trigger.querySelector('[data-icon="zap"]')).toBeTruthy();

    view.rerender(
      <ProviderControlSelect
        label="응답 속도"
        options={options}
        value="flex"
        showSpeedIcon
        onChange={vi.fn()}
      />
    );
    expect(trigger.querySelector('[data-icon="coins"]')).toBeTruthy();

    await userEvent.click(trigger);
    const listbox = screen.getByRole("listbox", { name: "응답 속도" });
    expect(within(listbox).getByRole("option", { name: "기본" }).querySelector("[data-icon]")).toBeNull();
    expect(within(listbox).getByRole("option", { name: "Fast" }).querySelector('[data-icon="zap"]')).toBeTruthy();
    expect(within(listbox).getByRole("option", { name: "Priority" }).querySelector('[data-icon="zap"]')).toBeTruthy();
    expect(within(listbox).getByRole("option", { name: "Flex" }).querySelector('[data-icon="coins"]')).toBeTruthy();
  });

  it("answers a fast click while options are still loading instead of ignoring it", () => {
    const onChange = vi.fn();
    render(
      <ProviderControlSelect label="모델" options={[]} value="" onChange={onChange} />
    );
    fastClick(screen.getByRole("combobox", { name: "모델" }));
    expect(screen.getByRole("status").textContent).toContain("모델 목록을 불러오는 중이에요.");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("keeps the model search focused on the first press so one click selects", async () => {
    const onChange = vi.fn();
    render(
      <ProviderControlSelect
        label="모델"
        options={[
          { value: "gemini-3.1-flash-lite", label: "Gemini 3.1 Flash Lite" },
          { value: "gemini-3-pro", label: "Gemini 3 Pro" },
          { value: "gemini-2.5-flash", label: "Gemini 2.5 Flash" },
        ]}
        value=""
        onChange={onChange}
      />
    );
    const user = userEvent.setup();
    await user.click(screen.getByRole("combobox", { name: "모델" }));
    const search = screen.getByRole("searchbox", { name: "모델 검색" });
    await user.type(search, "gemini 3.1");
    const option = screen.getByRole("option", { name: /Gemini 3\.1 Flash Lite/ });

    await user.pointer({ keys: "[MouseLeft>]", target: option });
    // The press must not move focus out of the search field before mouseup.
    expect(document.activeElement).toBe(search);
    await user.pointer({ keys: "[/MouseLeft]", target: option });

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith("gemini-3.1-flash-lite");
  });

  it("focuses the model search without scrolling the page under the menu", async () => {
    const focus = vi.spyOn(HTMLElement.prototype, "focus");
    render(
      <ProviderControlSelect
        label="모델"
        options={[
          { value: "gemini-3.1-flash-lite", label: "Gemini 3.1 Flash Lite" },
          { value: "gemini-3-pro", label: "Gemini 3 Pro" },
        ]}
        value=""
        onChange={vi.fn()}
      />
    );
    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
    // Focusing without preventScroll makes the browser scroll the document to
    // the fixed menu; the scroll handler then closes the menu on open.
    expect(focus.mock.calls.some(([options]) => options?.preventScroll === true)).toBe(true);
    focus.mockRestore();
  });
  it("drills into model families inside the same bounded menu", async () => {
    render(
      <ProviderControlSelect
        label="모델"
        options={[
          { value: "gemini/flash", label: "Gemini Flash", metadata: { family: "Gemini" } },
          { value: "gemini/pro", label: "Gemini Pro", metadata: { family: "Gemini" } },
          { value: "claude/sonnet", label: "Claude Sonnet", metadata: { family: "Claude" } },
          { value: "claude/opus", label: "Claude Opus", metadata: { family: "Claude" } },
        ]}
        value="gemini/flash"
        onChange={vi.fn()}
      />
    );

    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
    expect(screen.queryByText("모델 카탈로그")).toBeNull();
    expect(screen.queryByText("제공사 또는 모델 선택")).toBeNull();
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Gemini 제공사, 2개 모델" })
    );

    expect(screen.queryByRole("menu", { name: "모델 분류" })).toBeNull();
    expect(screen.getAllByRole("listbox")).toHaveLength(1);
    const firstModel = screen.getByRole("option", { name: "Gemini Flash" });
    expect(firstModel).toBeTruthy();
    expect(document.activeElement).toBe(firstModel);
    expect(screen.queryByRole("option", { name: "Claude Sonnet" })).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "모델 목록으로 돌아가기" }));
    expect(screen.getByRole("menu", { name: "모델 분류" })).toBeTruthy();
  });

  it("searches a large model catalog without traversing family menus", async () => {
    const onChange = vi.fn();
    const options = Array.from({ length: 100 }, (_, index) => ({
      value: `vendor/model-${index}`,
      label: `Model ${index}`,
      metadata: {
        family: index % 2 ? "Odd" : "Even",
        pricing: index % 10 === 0 ? "free" : "paid",
      },
    }));
    render(
      <ProviderControlSelect
        label="모델"
        options={options}
        value="vendor/model-0"
        onChange={onChange}
      />
    );

    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
    const search = screen.getByRole("searchbox", { name: "모델 검색" });
    expect(document.activeElement).toBe(search);
    await userEvent.type(search, "Model 99");
    const results = screen.getByRole("listbox", { name: "모델" });
    expect(within(results).queryByRole("option", { name: "Model 0 Free" })).toBeNull();
    await userEvent.click(within(results).getByRole("option", { name: "Model 99" }));

    expect(onChange).toHaveBeenCalledWith("vendor/model-99");
  });

  it("filters models only when catalog metadata confirms free pricing", async () => {
    const onChange = vi.fn();
    render(
      <ProviderControlSelect
        label="모델"
        options={[
          { value: "vendor/free", label: "Free Model", metadata: { pricing: "free" } },
          { value: "vendor/tier", label: "Free Tier Model", metadata: { pricing: "free_tier" } },
          { value: "vendor/paid", label: "Paid Model" },
        ]}
        value="vendor/paid"
        onChange={onChange}
      />
    );

    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
    await userEvent.click(screen.getByRole("button", { name: "무료 모델만 보기" }));
    const results = screen.getByRole("listbox", { name: "모델" });
    expect(within(results).getByRole("option", { name: "Free Model Free" })).toBeTruthy();
    expect(within(results).getByRole("option", { name: "Free Tier Model Free tier" })).toBeTruthy();
    expect(within(results).queryByRole("option", { name: "Paid Model" })).toBeNull();
  });

  it("filters the catalog by capabilities reported by the provider", async () => {
    render(
      <ProviderControlSelect
        label="모델"
        options={[
          {
            value: "vendor/vision-reasoning",
            label: "Vision Reasoning",
            metadata: { vision: true, reasoning: true },
          },
          { value: "vendor/text", label: "Text Only", metadata: {} },
        ]}
        value="vendor/text"
        onChange={vi.fn()}
      />
    );

    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
    await userEvent.click(screen.getByRole("button", { name: "비전 모델만 보기" }));
    const results = screen.getByRole("listbox", { name: "모델" });
    expect(within(results).getByRole("option", { name: /Vision Reasoning/ })).toBeTruthy();
    expect(within(results).queryByRole("option", { name: "Text Only" })).toBeNull();
  });

  it("shows verified model limits and capabilities beside the hovered model", async () => {
    render(
      <ProviderControlSelect
        label="모델"
        options={[
          {
            value: "vendor/reasoner",
            label: "Reasoner",
            metadata: {
              context_length: 128_000,
              max_output_tokens: 16_384,
              input_price_per_million: "0.55",
              output_price_per_million: "2.19",
              reasoning: true,
              vision: false,
              derive_description: true,
              training_policy: "사용될 수 있음 · opt-out 가능",
            },
          },
          { value: "vendor/other", label: "Other" },
        ]}
        value="vendor/other"
        onChange={vi.fn()}
      />
    );

    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
    expect(screen.queryByText("Reasoning")).toBeNull();
    await userEvent.hover(
      screen.getByRole("option", {
        name: "Reasoner Reasoning Context 128,000 · 입력 $0.55/M · 출력 $2.19/M",
      })
    );

    const detail = document.querySelector(".dc-agent-model-details-popover");
    expect(detail).toBeTruthy();
    expect(within(detail as HTMLElement).getByText("128,000 tokens")).toBeTruthy();
    expect(within(detail as HTMLElement).getByText("16,384 tokens")).toBeTruthy();
    expect(within(detail as HTMLElement).getByText("$0.55/M")).toBeTruthy();
    expect(within(detail as HTMLElement).getByText("$2.19/M")).toBeTruthy();
    expect(within(detail as HTMLElement).getAllByText("지원")).toHaveLength(1);
    expect(within(detail as HTMLElement).getAllByText("미지원")).toHaveLength(1);
    expect(within(detail as HTMLElement).getByText("사용될 수 있음 · opt-out 가능")).toBeTruthy();
  });
});

describe("whole-row menu sizing", () => {
  function renderWithRowHeight(height: number, count = 30) {
    const original = HTMLElement.prototype.getBoundingClientRect;
    HTMLElement.prototype.getBoundingClientRect = function () {
      const rect = original.call(this) as DOMRect;
      if (this.tagName === "BUTTON" && this.closest(".dc-agent-select-menu")) {
        return { ...rect, height } as DOMRect;
      }
      return rect;
    };
    const cleanupRect = () => {
      HTMLElement.prototype.getBoundingClientRect = original;
    };
    const options = Array.from({ length: count }, (_, index) => ({
      value: `m-${index}`,
      label: `Model ${index}`,
    }));
    render(
      <ProviderControlSelect label="모델" options={options} value="" onChange={vi.fn()} />
    );
    return cleanupRect;
  }

  it("sets the row height from a real row so the last one is not sliced", async () => {
    // A flat pixel cap left 6.44 rows visible and the 7th half-drawn, which read
    // as clipped rather than scrollable.
    const cleanupRect = renderWithRowHeight(34);
    try {
      await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
      const menu = document.querySelector(".dc-agent-select-menu") as HTMLElement;
      expect(menu.style.getPropertyValue("--dc-select-row")).toBe("34px");
    } finally {
      cleanupRect();
    }
  });

  it("uses the taller measurement when rows carry descriptions", async () => {
    const cleanupRect = renderWithRowHeight(50);
    try {
      await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
      const menu = document.querySelector(".dc-agent-select-menu") as HTMLElement;
      expect(menu.style.getPropertyValue("--dc-select-row")).toBe("50px");
    } finally {
      cleanupRect();
    }
  });

  it("re-measures when the observed menu changes size", async () => {
    // jsdom has no ResizeObserver, so stand one in and fire it the way a real
    // browser would when the filter shortens the list.
    const callbacks: Array<() => void> = [];
    (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
      constructor(callback: () => void) {
        callbacks.push(callback);
      }
      observe() {}
      disconnect() {}
    };
    const cleanupRect = renderWithRowHeight(34);
    try {
      await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
      const menu = document.querySelector(".dc-agent-select-menu") as HTMLElement;
      expect(menu.style.getPropertyValue("--dc-select-row")).toBe("34px");

      menu.style.setProperty("--dc-select-row", "999px");
      callbacks.forEach((callback) => callback());

      expect(menu.style.getPropertyValue("--dc-select-row")).toBe("34px");
    } finally {
      cleanupRect();
      delete (globalThis as unknown as { ResizeObserver?: unknown }).ResizeObserver;
    }
  });
});

it("shows exactly one full-access warning only while selected", async () => {
  const warning = "방에 있는 누구의 말이든 이 컴퓨터에서 승인 없이 명령으로 실행될 수 있어요.";
  const props = { label: "권한", options: [
    { value: "meeting_read_only", label: "대화 전용" },
    { value: "workspace_write", label: "작업 폴더 쓰기" },
    { value: "full_access", label: "전체 액세스" },
  ], onChange: vi.fn() };
  const view = render(<ProviderControlSelect {...props} value="full_access" />);
  expect(screen.getAllByText(warning)).toHaveLength(1);
  await userEvent.click(screen.getByRole("combobox", { name: "권한" }));
  await userEvent.click(screen.getByRole("option", { name: "작업 폴더 쓰기" }));
  expect(props.onChange).toHaveBeenCalledWith("workspace_write");
  view.rerender(<ProviderControlSelect {...props} value="workspace_write" />);
  expect(screen.queryByText(warning)).toBeNull();
});

it("walks the rows with arrow keys and presses the focused one with Enter", async () => {
  const user = userEvent.setup();
  const onChange = vi.fn();
  const permissions = [
    { value: "meeting_read_only", label: "대화 전용" },
    { value: "workspace_write", label: "작업 폴더 쓰기" },
    { value: "full_access", label: "전체 액세스" },
  ];
  const view = render(
    <ProviderControlSelect label="권한" options={permissions} value="workspace_write" onChange={onChange} />
  );
  await user.click(screen.getByRole("combobox", { name: "권한" }));
  expect(document.activeElement).toBe(screen.getByRole("option", { name: "작업 폴더 쓰기" }));
  await user.keyboard("{ArrowUp}{ArrowDown}{ArrowDown}{End}{Enter}");
  expect(onChange).toHaveBeenLastCalledWith("full_access");
  view.unmount();

  const models = Array.from({ length: 3 }, (_, index) => ({
    value: `vendor/model-${index}`,
    label: `Model ${index}`,
    metadata: { family: "Vendor" },
  }));
  render(<ProviderControlSelect label="모델" options={models} value="" onChange={onChange} />);
  await user.click(screen.getByRole("combobox", { name: "모델" }));
  await user.keyboard("Model{ArrowDown}{ArrowDown}{Enter}");
  expect(onChange).toHaveBeenLastCalledWith("vendor/model-1");
});
