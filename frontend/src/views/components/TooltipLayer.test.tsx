import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import TooltipLayer from "./TooltipLayer";

afterEach(cleanup);

it("shows one hint for a titled control and gives the native title back afterwards", () => {
  render(<><TooltipLayer /><button title="답장">a</button><button title="고정">b</button><p>밖</p></>);
  const [reply, pin] = screen.getAllByRole("button");
  fireEvent.pointerOver(reply);
  expect(screen.getByRole("tooltip").textContent).toBe("답장");
  expect(reply.hasAttribute("title")).toBe(false);
  fireEvent.pointerOver(pin);
  expect(screen.getAllByRole("tooltip")).toHaveLength(1);
  expect(screen.getByRole("tooltip").textContent).toBe("고정");
  expect(reply.getAttribute("title")).toBe("답장");
  fireEvent.pointerOver(screen.getByText("밖"));
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(pin.getAttribute("title")).toBe("고정");
});

it("stays out of the way of touch and of presses", () => {
  render(<><TooltipLayer /><button title="답장">a</button></>);
  const reply = screen.getByRole("button");
  fireEvent.pointerOver(reply, { pointerType: "touch" });
  expect(screen.queryByRole("tooltip")).toBeNull();
  fireEvent.pointerOver(reply);
  fireEvent.pointerDown(reply);
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(reply.getAttribute("title")).toBe("답장");
});

it("adds nothing to a control that already shows the same words", () => {
  render(<><TooltipLayer /><button title="Codex"><span>Codex</span><small>구독 에이전트</small></button></>);
  const tile = screen.getByRole("button");
  fireEvent.pointerOver(tile);
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(tile.hasAttribute("title")).toBe(false);
});
