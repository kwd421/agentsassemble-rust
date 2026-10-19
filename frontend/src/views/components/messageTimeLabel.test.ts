import { describe, expect, it } from "vitest";
import { messageTimeLabel } from "./MessageRow";

describe("messageTimeLabel", () => {
  const now = new Date(2026, 9, 20, 15, 0);
  it("shows only the time today, 어제 yesterday and the date before that", () => {
    expect(messageTimeLabel(new Date(2026, 9, 20, 9, 5).toISOString(), now)).toMatch(/^오전 9:05$/);
    expect(messageTimeLabel(new Date(2026, 9, 19, 23, 11).toISOString(), now)).toMatch(/^어제 오후 11:11$/);
    expect(messageTimeLabel(new Date(2026, 7, 21, 23, 11).toISOString(), now)).toMatch(/^2026\. 8\. 21\. 오후 11:11$/);
  });
  it("returns nothing for an unreadable time", () => {
    expect(messageTimeLabel("not a date", now)).toBe("");
  });
});
