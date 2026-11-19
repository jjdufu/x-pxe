// 联调（step-12 §5）：对真实 xpxe-control（webui-stack：Step 10 冒烟方式启动 + Step 7 离线夹具资产）跑完 8 步。
// 只在 `npx playwright test e2e/integration.spec.ts` 时运行（见 playwright.config.ts）。
import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test, type Locator, type Page } from "@playwright/test";

const ROOT = fileURLToPath(new URL("../..", import.meta.url));
const FIXTURES = `${ROOT}crates/xpxe-assets/tests/fixtures/ipxe/`;
const INITIAL = "Init-Admin-Passw0rd!";
const PASSWORD = "Webui-Admin-Passw0rd-2026!";
const MAC = "52:54:00:12:00:21";
const HOST = "web-node-01";

interface StackInfo {
  boot: string;
  mirror: string;
}

const stackInfo = (): StackInfo => JSON.parse(readFileSync(`${ROOT}target/webui-stack-it/stack.json`, "utf8")) as StackInfo;

const row = (page: Page, text: string): Locator => page.locator("tr[data-row]", { hasText: text });

async function rowAction(page: Page, text: string, item: string): Promise<void> {
  // 刚关闭的对话框会把焦点还给其触发器，可能顺带关掉刚打开的行菜单：菜单已收起时（aria-expanded≠true）重开
  const trigger = row(page, text).first().locator(".row-menu-btn");
  const entry = page.locator("[role=menu][data-state=open]").getByRole("menuitem", { name: item, exact: true });
  await expect(async () => {
    // 右下角 Toast 会盖住最右侧的行操作按钮：先逐个关闭
    for (const close of await page.getByRole("button", { name: "关闭通知" }).all()) await close.click({ timeout: 1000 }).catch(() => undefined);
    if ((await trigger.getAttribute("aria-expanded")) !== "true") await trigger.click({ timeout: 2000 });
    // 定位完成前菜单内容在视口外；先等它进入视口再点击（否则 Playwright 会滚动页面，菜单随之关闭）
    await expect(entry).toBeInViewport({ timeout: 2000 });
    await entry.click({ timeout: 2000 });
  }).toPass({ timeout: 20_000 });
}

async function packageList(page: Page): Promise<Record<string, string[]>> {
  const reply = await page.request.get("/api/v1/init-modules");
  const body = (await reply.json()) as { package_groups: { id: string; packages: string[] }[] };
  return Object.fromEntries(body.package_groups.map((group) => [group.id, group.packages]));
}

test.describe.configure({ mode: "serial" });

test("WebUI 联调 8 步", async ({ page }) => {
  const info = stackInfo();
  // 诊断：写操作失败时把状态码与错误体打到输出（不含令牌）
  page.on("response", (response) => {
    if (response.request().method() !== "GET" && response.url().includes("/api/v1/") && response.status() >= 400) {
      void response.text().then((body) => {
        console.log(`[api] ${response.request().method()} ${new URL(response.url()).pathname} → ${String(response.status())} ${body.slice(0, 300)}`);
      });
    }
  });

  await test.step("1. 初始管理员登录 → 强制改密", async () => {
    await page.goto("/");
    await expect(page).toHaveURL(/\/login/u);
    await page.getByRole("textbox", { name: "用户名" }).fill("admin");
    await page.getByRole("textbox", { name: "密码", exact: true }).fill(INITIAL);
    await page.getByRole("button", { name: "登录" }).click();
    const dialog = page.getByRole("dialog", { name: "请修改初始密码" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("textbox", { name: "当前密码" }).fill(INITIAL);
    await dialog.getByRole("textbox", { name: "新密码", exact: true }).fill(PASSWORD);
    await dialog.getByRole("textbox", { name: "确认新密码" }).fill(PASSWORD);
    await dialog.getByRole("button", { name: "修改密码" }).click();
    await expect(dialog).toBeHidden();
    await expect(page.getByRole("heading", { name: "总览" })).toBeVisible();
  });

  await test.step("2. 上传 iPXE 夹具 → 激活（跳过金丝雀并填写理由）→ 审计页出现记录", async () => {
    await page.getByRole("link", { name: "资产" }).click();
    const uploads: [string, string][] = [
      ["iPXE BIOS（undionly.kpxe）", "undionly.kpxe"],
      ["iPXE UEFI（ipxe.efi）", "snponly.efi"],
      ["iPXE UEFI（snponly.efi）", "snponly.efi"],
    ];
    for (const [title, file] of uploads) {
      const card = page.locator(".ui-card", { hasText: title });
      await card.getByRole("button", { name: "获取" }).click();
      await page.getByRole("menuitem", { name: "上传文件" }).click();
      const dialog = page.getByRole("dialog", { name: "上传资产" });
      await dialog.locator("input[type=file]").setInputFiles(`${FIXTURES}${file}`);
      await dialog.getByRole("button", { name: "上传", exact: true }).click();
      await expect(dialog).toBeHidden();
    }
    // Debian netboot 由联调栈预置（无法在浏览器内上传 InRelease/SHA256SUMS 三件套），这里只激活
    for (const kind of ["ipxe_undionly", "ipxe_efi_x64", "ipxe_snponly_x64", "debian_netboot_amd64"]) {
      await rowAction(page, kind, "激活");
      const confirm = page.getByRole("alertdialog", { name: "激活资产" });
      await expect(confirm.getByRole("button", { name: "激活" })).toBeDisabled();
      await confirm.getByRole("textbox", { name: "跳过金丝雀的理由" }).fill("联调：离线夹具");
      await confirm.getByRole("button", { name: "激活" }).click();
      await expect(confirm).toBeHidden();
      await expect(row(page, kind).first()).toContainText("已激活");
    }
    await page.getByRole("link", { name: "审计日志" }).click();
    await expect(row(page, "asset.activate").first()).toBeVisible();
  });

  await test.step("3. 由 nexus-lvm 新建 Profile → 改 NTP → 预览实时变化 → 差异可见 → 保存", async () => {
    await page.getByRole("link", { name: "装机 Profile" }).click();
    await page.getByRole("button", { name: "新建" }).first().click();
    const dialog = page.getByRole("dialog", { name: "新建装机 Profile" });
    await dialog.getByRole("textbox", { name: "名称" }).fill("web-lvm");
    await dialog.getByText("nexus-lvm", { exact: true }).click();
    await dialog.getByRole("button", { name: "新建" }).click();
    await expect(page).toHaveURL(/\/profiles\/\d+/u);
    const preview = page.getByTestId("preview-pane");
    await expect(preview).toContainText("d-i ");
    const host = page.locator('[data-path="/mirror/host_port"] input');
    await host.fill(info.mirror);
    await page.locator('[data-path="/mirror/directory"] input').fill("/debian");
    const ntp = page.locator('[data-path="/ntp"] input');
    await ntp.fill("10.9.8.7");
    await expect(preview).toContainText("clock-setup/ntp-server string 10.9.8.7");
    await ntp.fill("10.9.8.8");
    await expect(preview).toContainText("clock-setup/ntp-server string 10.9.8.8");
    await page.getByRole("button", { name: "差异" }).click();
    const diff = page.getByRole("dialog", { name: "与头版本的差异" });
    await expect(diff.getByTestId("diff-content")).toContainText("/ntp");
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "保存", exact: true }).click();
    const save = page.getByRole("dialog", { name: "保存新版本" });
    await save.getByRole("textbox", { name: "变更说明" }).fill("联调：NTP 与镜像源");
    await save.getByRole("button", { name: "保存" }).click();
    await expect(save).toBeHidden();
    await expect(page.getByText("头版本 v2")).toBeVisible();
  });

  await test.step("4. 由 deb13-init-scripts 新建计划 → 关闭 docs_fonts → 包消失、包数减 6 → 保存", async () => {
    const groups = await packageList(page);
    const fonts = groups.docs_fonts ?? [];
    expect(fonts).toHaveLength(6);
    await page.getByRole("link", { name: "初始化计划" }).click();
    await page.getByRole("button", { name: "新建" }).first().click();
    const dialog = page.getByRole("dialog", { name: "新建初始化计划" });
    await dialog.getByRole("textbox", { name: "名称" }).fill("web-init");
    await dialog.getByText("deb13-init-scripts", { exact: true }).click();
    await dialog.getByRole("button", { name: "新建" }).click();
    await expect(page).toHaveURL(/\/init-plans\/\d+/u);
    await page.locator(".module-list").getByRole("button", { name: /软件包/u }).click();
    const count = page.getByTestId("package-count");
    await expect(count).toHaveText(/^\d+$/u);
    const before = Number(await count.textContent());
    await page.getByRole("tab", { name: "本模块脚本" }).click();
    const script = page.locator(".sticky-preview [role=tabpanel]:not([hidden])");
    const probe = fonts.find((name) => name.startsWith("fonts-")) ?? fonts[0] ?? "";
    await expect(script).toContainText(probe);
    await page.getByText(/^docs_fonts（/u).click();
    await expect(count).toHaveText(String(before - 6));
    await expect(script).not.toContainText(probe);
    await page.getByRole("button", { name: "保存", exact: true }).click();
    const save = page.getByRole("dialog", { name: "保存新版本" });
    await save.getByRole("textbox", { name: "变更说明" }).fill("联调：关闭文档字体");
    await save.getByRole("button", { name: "保存" }).click();
    await expect(save).toBeHidden();
    await expect(page.getByText("头版本 v2")).toBeVisible();
  });

  await test.step("5. 引导网络选“复用现有 DHCP”→ 片段 Tabs 出现", async () => {
    await page.getByRole("link", { name: "引导网络" }).click();
    await page.getByRole("button", { name: "开始配置" }).click();
    await page.getByText("复用现有 DHCP", { exact: true }).click();
    await expect(page.getByRole("tablist", { name: "DHCP 片段" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "kea" })).toBeVisible();
    await page.getByRole("button", { name: "保存", exact: true }).first().click();
    const save = page.getByRole("dialog", { name: "保存配置" });
    await save.getByRole("textbox", { name: "变更说明" }).fill("联调：复用现有 DHCP");
    await save.getByRole("button", { name: "保存" }).click();
    await expect(save).toBeHidden();
  });

  await test.step("6. 新建节点 → arm → 徽章 Armed、倒计时开始", async () => {
    await page.getByRole("link", { name: "节点" }).click();
    await page.getByRole("button", { name: "新建节点" }).first().click();
    const dialog = page.getByRole("dialog", { name: "新建节点" });
    await dialog.getByRole("textbox", { name: "主机名" }).fill(HOST);
    await dialog.getByRole("textbox", { name: "MAC" }).fill(MAC);
    await dialog.getByRole("combobox", { name: "装机 Profile" }).click();
    await page.getByRole("option", { name: "web-lvm" }).click();
    await dialog.getByRole("combobox", { name: "初始化计划" }).click();
    await page.getByRole("option", { name: "web-init" }).click();
    await dialog.getByRole("button", { name: "登记" }).click();
    await expect(dialog).toBeHidden();
    await rowAction(page, HOST, "arm");
    const confirm = page.getByRole("alertdialog", { name: "确认 arm" });
    await confirm.getByRole("button", { name: "arm" }).click();
    await expect(confirm).toBeHidden();
    await expect(row(page, HOST)).toContainText("Armed");
    const countdown = row(page, HOST).getByTestId("countdown");
    const first = await countdown.textContent();
    await expect.poll(async () => countdown.textContent(), { timeout: 5000 }).not.toBe(first);
  });

  await test.step("7. simulate-install：页面无刷新地 Installing → Installed → Configuring → Ready；时间轴增长；搜索与选择保持", async () => {
    const search = page.locator("main").getByPlaceholder("搜索", { exact: true });
    await search.fill(HOST);
    await row(page, HOST).getByRole("checkbox").check();
    await row(page, HOST).getByRole("cell", { name: HOST }).click();
    const drawer = page.getByRole("dialog", { name: "节点详情" });
    await expect(drawer).toBeVisible();
    await drawer.getByRole("tab", { name: "事件" }).click();
    const timeline = drawer.getByTestId("node-timeline");
    const initial = await timeline.locator("li").count();
    let reloaded = false;
    page.on("framenavigated", (frame) => {
      if (frame === page.mainFrame()) reloaded = true;
    });
    const child = spawn("cargo", ["run", "-q", "-p", "xpxe-lab", "--", "simulate-install", "--boot-base", info.boot, "--mac", MAC], { cwd: ROOT, stdio: ["ignore", "pipe", "pipe"] });
    const output: string[] = [];
    child.stdout.on("data", (chunk: Buffer) => output.push(chunk.toString()));
    child.stderr.on("data", (chunk: Buffer) => output.push(chunk.toString()));
    const exited = new Promise<number | null>((resolve) => {
      child.on("exit", (code) => {
        resolve(code);
      });
    });
    for (const state of ["Installing", "Installed", "Configuring", "Ready"]) {
      await expect(row(page, HOST).getByText(state, { exact: true }), `等待 ${state}`).toBeVisible({ timeout: 120_000 });
    }
    expect(await exited, output.join("")).toBe(0);
    await expect.poll(async () => timeline.locator("li").count()).toBeGreaterThan(initial + 4);
    expect(reloaded).toBe(false);
    await page.keyboard.press("Escape");
    await expect(search).toHaveValue(HOST);
    await expect(row(page, HOST).getByRole("checkbox")).toBeChecked();
  });

  await test.step("8. 重新装机：不输入主机名时确认按钮不可用", async () => {
    await rowAction(page, HOST, "重新装机");
    const confirm = page.getByRole("alertdialog", { name: "重新装机" });
    const button = confirm.getByRole("button", { name: "重新装机" });
    await expect(button).toBeDisabled();
    await confirm.getByRole("textbox").fill("wrong-host");
    await expect(button).toBeDisabled();
    await confirm.getByRole("textbox").fill(HOST);
    await expect(button).toBeEnabled();
    await confirm.getByRole("button", { name: "取消" }).click();
  });
});
