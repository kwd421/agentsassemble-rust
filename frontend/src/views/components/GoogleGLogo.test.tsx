import { render } from "@testing-library/react";
import { expect, it } from "vitest";

import GoogleGLogo from "./GoogleGLogo";

it("renders the official colour G asset unstretched", () => {
  const { container, rerender } = render(<GoogleGLogo />);
  const image = container.querySelector("img");
  expect(image?.getAttribute("alt")).toBe("");
  expect(image?.getAttribute("width")).toBe("18");
  expect(image?.getAttribute("height")).toBe("18");
  expect(image?.getAttribute("src")).toContain("data:image/svg+xml");
  expect(image?.getAttribute("src")).toContain("FFC107");
  rerender(<GoogleGLogo size={20} />);
  expect(container.querySelector("img")?.getAttribute("width")).toBe("20");
});
