import { describe, expect, it } from "vitest";

import { ownServerMismatchText } from "./localOwnership";

describe("ownServerMismatchText", () => {
  it("names the account's own server and says this computer's server is not it", () => {
    expect(ownServerMismatchText("Nel Le의 PC", false))
      .toBe("내 서버 ‘Nel Le의 PC’가 꺼져 있어요. 이 컴퓨터의 서버는 내 서버가 아니에요.");
    expect(ownServerMismatchText("작업실 맥", true))
      .toBe("내 서버 ‘작업실 맥’이 다른 컴퓨터에서 실행 중이에요. 이 컴퓨터의 서버는 내 서버가 아니에요.");
  });

  it("keeps the plain notice when the server has no name", () => {
    expect(ownServerMismatchText(" ", false)).toBe("내 서버가 있는 컴퓨터가 꺼져 있어요.");
  });
});
