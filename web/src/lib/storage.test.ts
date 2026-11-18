import { afterEach, describe, expect, it, vi } from "vitest";
import { readItem, readJson, removeItem, writeItem, writeJson } from "./storage";

const isNumbers = (value: unknown): value is number[] => Array.isArray(value) && value.every((item) => typeof item === "number");

describe("storage", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    removeItem("k");
  });

  it("正常读写 localStorage", () => {
    writeJson("k", [1, 2]);
    expect(window.localStorage.getItem("k")).toBe("[1,2]");
    expect(readJson("k", isNumbers)).toEqual([1, 2]);
  });

  it("localStorage 抛异常时退化为内存，不向外抛出", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("quota", "QuotaExceededError");
    });
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("denied", "SecurityError");
    });
    expect(() => {
      writeItem("k", "v");
    }).not.toThrow();
    expect(readItem("k")).toBe("v");
  });

  it("访问 window.localStorage 本身抛异常（隐私模式）时退化为内存", () => {
    vi.spyOn(window, "localStorage", "get").mockImplementation(() => {
      throw new DOMException("denied", "SecurityError");
    });
    writeItem("k", "memory");
    expect(readItem("k")).toBe("memory");
  });

  it("JSON 损坏或形状不符时返回 null", () => {
    window.localStorage.setItem("k", "{oops");
    expect(readJson("k", isNumbers)).toBeNull();
    window.localStorage.setItem("k", '{"a":1}');
    expect(readJson("k", isNumbers)).toBeNull();
  });
});
