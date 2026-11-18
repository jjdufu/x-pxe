// 公共夹具：拦截并统计所有非同源请求（离线断言），用路由拦截模拟管理 API 的四种状态。
import { test as base, expect, type Page, type Route } from "@playwright/test";

export type ApiState = "populated" | "empty" | "loading" | "error";

export const PAGES: { path: string; name: string; endpoint: string }[] = [
  { path: "/", name: "总览", endpoint: "/api/v1/system/info" },
  { path: "/nodes", name: "节点", endpoint: "/api/v1/nodes" },
  { path: "/profiles", name: "装机 Profile", endpoint: "/api/v1/configs/profile" },
  { path: "/init-plans", name: "初始化计划", endpoint: "/api/v1/configs/init_plan" },
  { path: "/boot-network", name: "引导网络", endpoint: "/api/v1/bootnet/status" },
  { path: "/assets", name: "资产", endpoint: "/api/v1/assets" },
  { path: "/history", name: "配置历史", endpoint: "/api/v1/configs/profile" },
  { path: "/audit", name: "审计日志", endpoint: "/api/v1/audit/verify" },
  { path: "/settings", name: "设置", endpoint: "/api/v1/me" },
];

const NODES = Array.from({ length: 6 }, (_, index) => ({
  id: index + 1,
  hostname: `node-${String(index + 1).padStart(2, "0")}`,
  mac: `52:54:00:00:00:${String(index + 1).padStart(2, "0")}`,
  state: ["Idle", "Armed", "Installing", "Ready", "Failed", "Ready"][index] ?? "Idle",
  channel: "lan",
  firmware: index % 2 === 0 ? "uefi" : "bios",
  state_since: "2026-11-18T12:00:00Z",
}));

const POPULATED: Record<string, unknown> = {
  "/api/v1/system/info": { version: "0.1.0", boot_base_url: "http://10.0.0.2:8080", https: false, offline: true },
  "/api/v1/nodes": NODES,
  "/api/v1/configs/profile": [{ id: 1, name: "default", version: 3 }],
  "/api/v1/configs/init_plan": [{ id: 2, name: "deb13-init-scripts", version: 1 }],
  "/api/v1/bootnet/status": { healthy: true, rev: 4, heartbeat: { at: "2026-11-18T12:00:00Z" } },
  "/api/v1/assets": [{ id: 1, kind: "ipxe", stage: "active" }],
  "/api/v1/audit/verify": { ok: true, count: 42, first_broken: null },
  "/api/v1/me": { user: { id: 1, username: "admin", role: "admin", must_change_password: false }, role: "admin" },
};

const EMPTY: Record<string, unknown> = {
  "/api/v1/nodes": [],
  "/api/v1/configs/profile": [],
  "/api/v1/configs/init_plan": [],
  "/api/v1/assets": [],
  "/api/v1/system/info": null,
  "/api/v1/bootnet/status": null,
  "/api/v1/audit/verify": null,
  "/api/v1/me": null,
};

/** 安装 API 拦截：target 端点按 state 返回，其他端点返回正常数据；SSE 返回空流。 */
export async function mockApi(page: Page, state: ApiState = "populated", target?: string): Promise<{ calls: () => number }> {
  let calls = 0;
  await page.route("**/api/v1/**", async (route: Route) => {
    const url = new URL(route.request().url());
    if (url.pathname === "/api/v1/events/stream") {
      await route.fulfill({ status: 200, headers: { "content-type": "text/event-stream" }, body: "retry: 600000\n\n" });
      return;
    }
    const isTarget = target === undefined || url.pathname === target;
    if (isTarget && url.pathname === target) calls += 1;
    const mode = isTarget ? state : "populated";
    if (mode === "loading") return; // 永不应答
    if (mode === "error") {
      await route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: { code: "INTERNAL", message: "控制面内部错误" } }) });
      return;
    }
    const table = mode === "empty" ? EMPTY : POPULATED;
    const body = url.pathname in table ? table[url.pathname] : POPULATED[url.pathname] ?? null;
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
  });
  return { calls: () => calls };
}

interface Fixtures {
  external: string[];
}

/** 每个用例都统计非同源请求并在结束时断言为 0（门禁第 6 项）。 */
export const test = base.extend<Fixtures>({
  external: async ({ page, baseURL }, use) => {
    const origin = new URL(baseURL ?? "http://127.0.0.1:4173").origin;
    const external: string[] = [];
    await page.route(/.*/u, async (route) => {
      const url = route.request().url();
      if (url.startsWith("data:") || url.startsWith("blob:") || new URL(url).origin === origin) {
        await route.fallback();
        return;
      }
      external.push(url);
      await route.abort();
    });
    await use(external);
    expect(external, `非同源请求：${external.join(", ")}`).toHaveLength(0);
  },
});

export { expect };
