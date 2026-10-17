import { describe, expect, it } from "vitest";
import { emptyRoomCopy } from "./AppView";

describe("emptyRoomCopy", () => {
  it("promises room creation only when this shell can create rooms", () => {
    expect(emptyRoomCopy(true)).toEqual({
      title: "열려 있는 방이 없어요",
      description: "새 방을 만들어 대화를 시작할 수 있어요.",
    });
  });

  it("points a non-owner shell at joining through an invite link", () => {
    const copy = emptyRoomCopy(false);
    expect(copy.title).toBe("참가한 방이 없어요");
    expect(copy.description).toBe("초대 링크로 참가할 수 있어요.");
    expect(`${copy.title} ${copy.description}`).not.toContain("새 방");
    for (const line of [copy.title, copy.description]) {
      expect(line.trim().split("\n")).toHaveLength(1);
    }
  });
});
