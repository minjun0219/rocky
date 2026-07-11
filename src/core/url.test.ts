import { describe, it, expect } from "bun:test";
import { joinBaseAndPath, syntheticOperationId } from "./url";

describe("joinBaseAndPath", () => {
  it("inserts a slash when missing", () => {
    expect(joinBaseAndPath("https://api.example.com", "pet")).toBe(
      "https://api.example.com/pet",
    );
  });
  it("strips trailing slashes from baseUrl and keeps the path verbatim", () => {
    // baseUrl 끝의 `/` 를 모두 제거한 뒤 path 를 그대로 이어 붙인다 — path 안의
    // `//` 는 정규화 대상이 아님 (OpenAPI path 가 의도해서 비워 둔 segment 일 수 있어
    // joinBaseAndPath 가 자의적으로 합치지 않는다).
    expect(joinBaseAndPath("https://api.example.com/", "/pet")).toBe(
      "https://api.example.com/pet",
    );
    expect(joinBaseAndPath("https://api.example.com//", "//pet")).toBe(
      "https://api.example.com//pet",
    );
  });
  it("preserves path-style base URLs", () => {
    expect(joinBaseAndPath("https://api.example.com/v1", "/pet/{petId}")).toBe(
      "https://api.example.com/v1/pet/{petId}",
    );
  });
  it("returns path as-is when baseUrl is empty", () => {
    expect(joinBaseAndPath("", "/pet")).toBe("/pet");
  });
});

describe("syntheticOperationId", () => {
  it("drops braces and lowercases method", () => {
    expect(syntheticOperationId("GET", "/pet/{petId}")).toBe("get_pet_petId");
  });
  it("handles root path", () => {
    expect(syntheticOperationId("post", "/")).toBe("post_root");
  });
});
