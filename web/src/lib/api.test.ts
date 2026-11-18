import { afterEach, describe, expect, it, vi } from "vitest";
import { api, ApiError, csrfToken } from "./api";

describe("api", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    document.cookie = "xpxe_csrf=; max-age=0";
  });

  it("从 xpxe_csrf Cookie 读回 CSRF 令牌", () => {
    expect(csrfToken("a=1; xpxe_csrf=tok%3D1; b=2")).toBe("tok=1");
    expect(csrfToken("a=1")).toBeNull();
  });

  it("写操作带 X-CSRF-Token，读操作不带", async () => {
    document.cookie = "xpxe_csrf=abc";
    const fetchMock = vi.fn<(url: string, init: RequestInit) => Promise<Response>>(() => Promise.resolve(new Response("{}", { status: 200 })));
    vi.stubGlobal("fetch", fetchMock);
    await api("/nodes");
    await api("/nodes/1/arm", { method: "POST", body: {} });
    const [getCall, postCall] = fetchMock.mock.calls;
    expect(new Headers(getCall?.[1].headers).get("X-CSRF-Token")).toBeNull();
    expect(new Headers(postCall?.[1].headers).get("X-CSRF-Token")).toBe("abc");
    expect(getCall?.[0]).toBe("/api/v1/nodes");
  });

  it("错误响应转成带错误码的 ApiError", async () => {
    vi.stubGlobal("fetch", () => Promise.resolve(new Response(JSON.stringify({ error: { code: "UNAUTHENTICATED", message: "需要登录" } }), { status: 401 })));
    await expect(api("/me")).rejects.toMatchObject({ status: 401, code: "UNAUTHENTICATED", message: "需要登录" });
  });

  it("网络错误转成 NETWORK", async () => {
    vi.stubGlobal("fetch", () => Promise.reject(new TypeError("Failed to fetch")));
    const error: unknown = await api("/me").catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe("NETWORK");
  });
});
