import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { copyText } from "../../lib/clipboard";
import { CopyButton } from "./code";

describe("CopyButton / copyText", () => {
  const setClipboard = (value: unknown) => {
    Object.defineProperty(navigator, "clipboard", { value, configurable: true });
  };

  afterEach(() => {
    vi.restoreAllMocks();
    setClipboard(undefined);
  });

  it("navigator.clipboard 不存在时走 textarea + execCommand('copy')", async () => {
    setClipboard(undefined);
    const exec = vi.fn(() => true);
    Object.defineProperty(document, "execCommand", { value: exec, configurable: true });
    render(<CopyButton value="sha256:abc" />);
    await userEvent.click(screen.getByRole("button", { name: "复制" }));
    await waitFor(() => {
      expect(exec).toHaveBeenCalledWith("copy");
    });
    expect(document.querySelector("textarea")).toBeNull();
    expect(await screen.findByText("已复制")).toBeTruthy();
  });

  it("writeText 被拒绝时同样降级", async () => {
    setClipboard({ writeText: vi.fn(() => Promise.reject(new Error("denied"))) });
    Object.defineProperty(document, "execCommand", { value: vi.fn(() => true), configurable: true });
    await expect(copyText("x")).resolves.toBe("fallback");
  });

  it("优先使用异步剪贴板 API", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    setClipboard({ writeText });
    await expect(copyText("x")).resolves.toBe("clipboard");
    expect(writeText).toHaveBeenCalledWith("x");
  });

  it("两种方式都失败时返回 failed", async () => {
    setClipboard(undefined);
    Object.defineProperty(document, "execCommand", { value: vi.fn(() => false), configurable: true });
    await expect(copyText("x")).resolves.toBe("failed");
  });
});
