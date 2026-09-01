import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import MemberUsage from "./MemberUsage";

afterEach(cleanup);

describe("MemberUsage", () => {
  it("reports that exact provider usage is unsupported", () => {
    render(<MemberUsage displayName="Agent One" />);

    expect(screen.getByRole("region", { name: "Agent One 사용량" })).toBeTruthy();
    expect(screen.getByText(/정확한 잔여량을 제공하지 않습니다/)).toBeTruthy();
  });
});
