// 公共夹具：拦截并统计所有非同源请求（离线断言），用路由拦截模拟管理 API 的四种状态。
import { readFileSync } from "node:fs";
import { test as base, expect, type Page, type Route } from "@playwright/test";

export type ApiState = "populated" | "empty" | "loading" | "error";

export const PAGES: { path: string; name: string; endpoint: string }[] = [
  { path: "/", name: "总览", endpoint: "/api/v1/overview" },
  { path: "/nodes", name: "节点", endpoint: "/api/v1/nodes" },
  { path: "/profiles", name: "装机 Profile", endpoint: "/api/v1/configs/profile" },
  { path: "/init-plans", name: "初始化计划", endpoint: "/api/v1/configs/init_plan" },
  { path: "/boot-network", name: "引导网络", endpoint: "/api/v1/configs/boot_network" },
  { path: "/assets", name: "资产", endpoint: "/api/v1/assets" },
  { path: "/history", name: "配置历史", endpoint: "/api/v1/history" },
  { path: "/audit", name: "审计日志", endpoint: "/api/v1/audit" },
  { path: "/settings", name: "设置", endpoint: "/api/v1/me" },
];

/** 控制面真实响应样本（webui-stack 抓取，已去除口令哈希）。 */
interface MockData {
  init_modules: unknown;
  presets: unknown;
  profile_head: { document: Record<string, unknown>; version: unknown };
  plan_head: unknown;
  snippets: unknown;
  profile_preview: { artifacts: { name: string; text: string }[] };
  plan_preview: unknown;
}
const DATA = JSON.parse(readFileSync(new URL("mock-data.json", import.meta.url), "utf8")) as MockData;

const NOW = 1795060000;
const STATES = ["idle", "armed", "installing", "ready", "failed", "ready"] as const;
const NODES = STATES.map((state, index) => {
  const hostname = `node-${String(index + 1).padStart(2, "0")}`;
  const mac = `52:54:00:00:00:${String(index + 1).padStart(2, "0")}`;
  const spec = { hostname, fqdn: null, mac, os_family: "debian13", firmware: index % 2 === 0 ? "uefi" : "bios", channel: { type: "lan" }, network: { type: "dhcp" } };
  return {
    id: index + 1,
    hostname,
    mac,
    state,
    state_since: NOW - 600,
    deadline_at: state === "armed" || state === "installing" ? NOW + 3600 : null,
    active_session_id: state === "installing" ? "sess-3" : null,
    channel: { type: "lan" },
    firmware: spec.firmware,
    network: { type: "dhcp" },
    profile_id: 1,
    init_plan_id: 2,
    alert: null,
    last_event_id: null,
    spec,
    spec_object_id: 10 + index,
  };
});

const EVENTS = [
  { id: 1, node_id: 3, hostname: "node-03", session_id: "sess-3", event_id: "e1", at: NOW - 300, source: "control", kind: "armed", from_state: "idle", to_state: "armed", payload: {} },
  { id: 2, node_id: 3, hostname: "node-03", session_id: "sess-3", event_id: "e2", at: NOW - 200, source: "installer", kind: "installer.started", from_state: "armed", to_state: "installing", payload: {} },
];

const POPULATED: Record<string, unknown> = {
  "/api/v1/system/info": { version: "0.1.0", boot_base_url: "http://10.0.0.2:8080", https: false, offline: true, admin_listen: "0.0.0.0:8088", data_dir: "/var/lib/xpxe", master_key_file: "/etc/xpxe/master.key", bootnet_allowed_ifaces: ["br-lab"], tls_cert_file: null },
  "/api/v1/me": { user: { id: 1, username: "admin", role: "admin", must_change_password: false }, role: "admin" },
  "/api/v1/overview": { now: NOW, states: { by_state: { idle: 1, armed: 1, installing: 1, ready: 2, failed: 1 }, idle: 1 }, outcomes: [{ day: "2026-11-18", ready: 3, failed: 1 }], events: EVENTS },
  "/api/v1/nodes": NODES,
  "/api/v1/nodes/3/timeline": EVENTS,
  "/api/v1/configs/profile": [{ id: 1, kind: "profile", name: "nexus-lvm", head_version: 2 }],
  "/api/v1/configs/init_plan": [{ id: 2, kind: "init_plan", name: "deb13-init-scripts", head_version: 1 }],
  "/api/v1/configs/boot_network": [{ id: 4, kind: "boot_network", name: "default", head_version: 1 }],
  "/api/v1/configs/dhcp_rules": [],
  "/api/v1/configs/timeouts": [],
  "/api/v1/configs/profile/1": { ...DATA.profile_head, version: { object_id: 1, version: 2, content_sha256: "a".repeat(64) } },
  "/api/v1/configs/init_plan/2": DATA.plan_head,
  "/api/v1/configs/boot_network/4": { document: { mode: "proxy_dhcp", iface: "br-lab" }, version: { object_id: 4, version: 1, content_sha256: "b".repeat(64) } },
  "/api/v1/presets": DATA.presets,
  "/api/v1/init-modules": DATA.init_modules,
  "/api/v1/bootnet/status": {
    healthy: true,
    rev: 4,
    heartbeat: { received_at: NOW, heartbeat: { at: NOW, version: "0.1.0", rev: 4, mode: "proxy", health: "ok", detail: null, listening: [67, 69, 4011], counters: { dhcp_received: 12, dhcp_replies: 6 }, probe: { at: NOW, responders: [{ server: "10.0.0.1", class: "address_only", siaddr: "0.0.0.0", file: null, options: [1, 3, 6] }], conflicts: [], external: null } } },
  },
  "/api/v1/bootnet/observations": [{ at: NOW - 30, mac: "52:54:00:aa:bb:cc", arch: "x64-uefi", user_class: "", kind: "unknown_mac", detail: "未登记的 MAC" }],
  "/api/v1/bootnet/snippets": DATA.snippets,
  "/api/v1/assets": {
    assets: [
      { id: 1, kind: "ipxe_efi_x64", label: "ipxe.efi v1.21.1", sha256: "9a624b4e05ebc25718a7658356628a352532bd278e7a50465a36ab94a7c4e580", size: 284672, source: "upload", source_url: null, kernel_abi: null, verify: { version: "v1.21.1" }, stage: "verified", created_at: NOW - 900, created_by: "admin" },
      { id: 2, kind: "debian_netboot_amd64", label: "trixie netboot", sha256: "ed3ddaf66df30b225ce1565d14abebd78fc6d1c82230f589ce22bbf2422c8351", size: 52428800, source: "official", source_url: null, kernel_abi: "6.12.57+deb13-amd64", verify: null, stage: "verified", created_at: NOW - 800, created_by: "admin" },
    ],
    channels: { channels: { ipxe_efi_x64: { asset_id: 1, sha256: "9a624b4e05ebc25718a7658356628a352532bd278e7a50465a36ab94a7c4e580" } } },
  },
  "/api/v1/history": [
    { kind: "profile", object_id: 1, name: "nexus-lvm", head_version: 2, version: 2, parent_version: 1, author: "admin", created_at: NOW - 100, comment: "调整 NTP", content_sha256: "a".repeat(64) },
    { kind: "profile", object_id: 1, name: "nexus-lvm", head_version: 2, version: 1, parent_version: null, author: "admin", created_at: NOW - 1000, comment: "", content_sha256: "c".repeat(64) },
  ],
  "/api/v1/audit": {
    total: 2,
    actors: ["admin"],
    actions: ["auth.login", "node.armed"],
    entries: [
      { id: 2, at: "2026-11-18T12:00:00Z", actor: "admin", ip: "10.0.0.5", action: "node.armed", target: "node:2", outcome: "ok", detail: {} },
      { id: 1, at: "2026-11-18T11:59:00Z", actor: "admin", ip: "10.0.0.5", action: "auth.login", target: "user:1", outcome: "ok", detail: {} },
    ],
  },
  "/api/v1/audit/verify": { checked: 42, first_broken: null },
  "/api/v1/users": [{ id: 1, username: "admin", role: "admin", disabled: false, must_change_password: false, created_at: NOW - 86400 }],
  "/api/v1/me/sessions": [{ key: "77ba047a3f1aab11", created_at: NOW - 600, last_seen_at: NOW, expires_at: NOW + 43200, ip: "10.0.0.5", user_agent: "Mozilla/5.0", current: true }],
  "/api/v1/me/tokens": [{ id: 1, name: "ci", prefix: "xpxe_ab12", scopes: ["read"], created_at: NOW - 600, expires_at: NOW + 86400 * 90 }],
  "/api/v1/bmcs": [],
};

const EMPTY: Record<string, unknown> = {
  "/api/v1/overview": null,
  "/api/v1/nodes": [],
  "/api/v1/configs/profile": [],
  "/api/v1/configs/init_plan": [],
  "/api/v1/configs/boot_network": [],
  "/api/v1/assets": { assets: [], channels: { channels: {} } },
  "/api/v1/history": [],
  "/api/v1/audit": null,
  "/api/v1/me": null,
};

/** 预览：回显上下文与 NTP，便于断言“改动 → 预览更新”。 */
function previewOf(kind: string, body: unknown): unknown {
  if (kind === "init_plan") return DATA.plan_preview;
  const request = (body ?? {}) as { document?: { ntp?: string[] }; context?: string };
  const ntp = (request.document?.ntp ?? []).join(" ");
  const echo = `# context=${request.context ?? "dhcp"}\nd-i clock-setup/ntp-server string ${ntp}\n`;
  return { ok: true, fields: [], artifacts: DATA.profile_preview.artifacts.map((item) => (item.name === "preseed.cfg" ? { ...item, text: echo + item.text } : item)), changes: [], rendered_diff: [] };
}

export interface ApiMock {
  calls: () => number;
  /** 收到的写请求（方法 + 路径）。 */
  writes: () => string[];
}

/** 安装 API 拦截：target 端点按 state 返回，其他端点返回正常数据；SSE 返回空流；写请求记录后回 200。 */
export async function mockApi(page: Page, state: ApiState = "populated", target?: string): Promise<ApiMock> {
  let calls = 0;
  const writes: string[] = [];
  await page.route("**/api/v1/**", async (route: Route) => {
    const request = route.request();
    const url = new URL(request.url());
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
    const preview = /^\/api\/v1\/configs\/(\w+)\/preview$/u.exec(url.pathname);
    if (preview !== null) {
      await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(previewOf(preview[1] ?? "", request.postDataJSON())) });
      return;
    }
    if (request.method() !== "GET") {
      writes.push(`${request.method()} ${url.pathname}`);
      await route.fulfill({ status: 200, contentType: "application/json", body: "{}" });
      return;
    }
    const table = mode === "empty" ? EMPTY : POPULATED;
    const body = url.pathname in table ? table[url.pathname] : (POPULATED[url.pathname] ?? null);
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
  });
  return { calls: () => calls, writes: () => [...writes] };
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
