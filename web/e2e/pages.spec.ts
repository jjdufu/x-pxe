// step-12 验收：业务页面的键盘流程（节点页、Profile 页）与编辑器渲染（路由拦截，离线）。
import type { Locator, Page } from "@playwright/test";
import { expect, mockApi, test } from "./fixtures";

async function tabTo(page: Page, target: Locator): Promise<void> {
  for (let step = 0; step < 200; step += 1) {
    await page.keyboard.press("Tab");
    if (await target.evaluate((element) => element === document.activeElement || element.contains(document.activeElement))) return;
  }
  throw new Error("Tab 无法到达目标元素");
}

test("节点页：搜索 → 方向键选行 → 菜单 → arm → AlertDialog 确认", async ({ page, external }) => {
  const api = await mockApi(page);
  await page.goto("/nodes");
  const search = page.locator("main").getByPlaceholder("搜索", { exact: true });
  await search.click();
  await page.keyboard.type("node-0");
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("button", { name: "操作 1" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("button", { name: "操作 2" })).toBeFocused();
  await page.keyboard.press("ArrowUp");
  const trigger = page.getByRole("button", { name: "操作 1" });
  await expect(trigger).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(trigger).toHaveAttribute("aria-expanded", "true");
  await expect(page.locator("[role=menu][data-state=open] [role=menuitem][data-highlighted]")).toHaveText("arm");
  // 等焦点移入菜单再按 Enter，避免与 Ark 打开后的聚焦竞争
  await expect(page.locator(":focus")).toHaveAttribute("role", "menu");
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("alertdialog", { name: "确认 arm" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("button", { name: "取消" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(dialog.getByRole("button", { name: "arm" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(dialog).toBeHidden();
  expect(api.writes()).toContain("POST /api/v1/nodes/1/arm");
  expect(external).toHaveLength(0);
});

test("Profile 页：Tab 到预览上下文 Select → ↓ → Enter → 预览更新", async ({ page, external }) => {
  await mockApi(page);
  await page.goto("/profiles/1");
  const preview = page.getByTestId("preview-pane");
  await expect(preview).toContainText("context=dhcp");
  const select = page.getByRole("combobox", { name: "预览上下文" });
  await tabTo(page, select);
  await page.keyboard.press("Enter");
  await expect(select).toHaveAttribute("aria-expanded", "true");
  // 打开后由 Ark 高亮当前项，再按方向键
  await expect(page.locator("[role=listbox][data-state=open] [role=option][data-highlighted]")).toHaveCount(1);
  await expect(page.locator(":focus")).toHaveAttribute("role", "listbox");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(select).toContainText("静态地址样例");
  await expect(preview).toContainText("context=static");
  expect(external).toHaveLength(0);
});

test("Profile 页：修改 NTP 后预览实时变化、出现未保存徽章", async ({ page, external }) => {
  await mockApi(page);
  await page.goto("/profiles/1");
  const ntp = page.locator('[data-path="/ntp"] input');
  await ntp.fill("10.9.8.7, 10.9.8.8");
  await expect(page.getByTestId("preview-pane")).toContainText("clock-setup/ntp-server string 10.9.8.7 10.9.8.8");
  await expect(page.getByText("未保存")).toBeVisible();
  expect(external).toHaveLength(0);
});

test("初始化计划编辑器：模块列表、包数、本模块脚本", async ({ page, external }) => {
  await mockApi(page);
  await page.goto("/init-plans/2");
  await expect(page.getByTestId("package-count")).toHaveText(/^\d+$/u);
  await page.locator(".module-list").getByRole("button", { name: /软件包/u }).click();
  await expect(page.locator(".sticky-preview [role=tabpanel]:not([hidden])")).toContainText("apt-get install");
  expect(external).toHaveLength(0);
});

test("引导网络：切换到“复用现有 DHCP”出现片段 Tabs", async ({ page, external }) => {
  await mockApi(page);
  await page.goto("/boot-network");
  await page.getByText("复用现有 DHCP", { exact: true }).click();
  await expect(page.getByRole("tablist", { name: "DHCP 片段" })).toBeVisible();
  expect(external).toHaveLength(0);
});
