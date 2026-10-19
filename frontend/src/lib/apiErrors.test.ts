import { describe, expect, it } from "vitest";
import { ApiError, userFacingError } from "./apiErrors";

describe("userFacingError", () => {
  it("falls back for transport failures like WebKit Load failed", () => {
    expect(userFacingError(new TypeError("Load failed"), "사진을 불러오지 못했어요.")).toBe(
      "사진을 불러오지 못했어요.",
    );
    expect(userFacingError(new TypeError("Failed to fetch"), "서버에 연결하지 못했어요.")).toBe(
      "서버에 연결하지 못했어요.",
    );
  });

  it("falls back for AbortError, SyntaxError and English server text", () => {
    expect(
      userFacingError(new DOMException("Fetch is aborted", "AbortError"), "다시 시도해 주세요."),
    ).toBe("다시 시도해 주세요.");
    expect(userFacingError(new SyntaxError("Unexpected token"), "다시 시도해 주세요.")).toBe(
      "다시 시도해 주세요.",
    );
    expect(userFacingError(new ApiError(500, "Internal Server Error"), "다시 시도해 주세요.")).toBe(
      "다시 시도해 주세요.",
    );
    expect(userFacingError(undefined, "다시 시도해 주세요.")).toBe("다시 시도해 주세요.");
  });

  it("keeps product-authored Korean messages", () => {
    expect(
      userFacingError(new Error("로그인 서버에 연결할 수 없어요."), "다시 시도해 주세요."),
    ).toBe("로그인 서버에 연결할 수 없어요.");
  });

  it("falls back for formal internal check messages", () => {
    expect(userFacingError(new Error("현재 확인된 로컬 방 관리자 권위가 없습니다."), "초대를 만들지 못했어요.")).toBe("초대를 만들지 못했어요.");
    expect(userFacingError(new Error("이름은 32자까지 입력할 수 있어요."), "x")).toBe("이름은 32자까지 입력할 수 있어요.");
  });

  it("falls back when a Korean line carries internal ids", () => {
    expect(
      userFacingError(
        new Error("중앙 오류 server_id 550e8400-e29b-41d4-a716-446655440000 2026-10-05T12:00:00Z"),
        "참가를 마치지 못했어요.",
      ),
    ).toBe("참가를 마치지 못했어요.");
  });

  it("maps known server codes to Korean", () => {
    expect(
      userFacingError(
        new ApiError(409, "conflict", "duplicate_resolution_conflict"),
        "다시 시도해 주세요.",
      ),
    ).toBe("다른 기기에서 서버를 선택했어요. 목록을 다시 확인해 주세요.");
    expect(
      userFacingError(new ApiError(401, "nope", "provider_login_missing"), "로그인하지 못했어요."),
    ).toBe("AI CLI를 설치한 뒤 로그인해 주세요.");
  });
});
