// 门禁第 8 项：仅用 Tab/↑/↓/Enter/ESC 完成 Select、Combobox 多选、Menu、命令面板、Dialog、AlertDialog、Drawer；
// 每个可聚焦元素有可见 :focus-visible 环（--ring → box-shadow）。
import type { Locator, Page } from "@playwright/test";
import { expect, mockApi, test } from "./fixtures";

/** 按 Tab 直到焦点落在目标上（最多 200 次）。 */
async function tabTo(page: Page, target: Locator): Promise<void> {
  for (let step = 0; step < 200; step += 1) {
    await page.keyboard.press("Tab");
    if (await target.evaluate((element) => element === document.activeElement || element.contains(document.activeElement))) return;
  }
  throw new Error("Tab 无法到达目标元素");
}

async function hasRing(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const element = document.activeElement;
    if (element === null) return false;
    const own = getComputedStyle(element).boxShadow;
    // Ark 把焦点状态标在控件上（data-focus-visible），检查自身或最近的可视控件
    const control = element.closest("[data-focus-visible], .ui-switch, .radio-card, .stepper");
    const other = control === null ? "none" : getComputedStyle(control).boxShadow;
    return own !== "none" || other !== "none";
  });
}

test.beforeEach(async ({ page }) => {
  await mockApi(page);
  await page.goto("/gallery.html");
});

test("Select：Enter 打开、↓ 选择、Enter 确认、ESC 关闭", async ({ page, external }) => {
  const trigger = page.getByRole("combobox", { name: "引导方式" });
  await tabTo(page, trigger);
  expect(await hasRing(page)).toBe(true);
  await page.keyboard.press("Enter");
  await expect(trigger).toHaveAttribute("aria-expanded", "true");
  // 等焦点真正移入浮层再按方向键，避免与 Ark 打开动画/聚焦竞争
  await expect(page.locator(":focus")).toHaveAttribute("role", "listbox");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  await expect(trigger).toContainText("BIOS PXE");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("option", { name: "BIOS PXE" })).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Escape");
  await expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(external).toHaveLength(0);
});

test("Combobox 多选：输入过滤 + Enter 选中多个", async ({ page, external }) => {
  const input = page.getByRole("combobox", { name: "时区" });
  await tabTo(page, input);
  await page.keyboard.type("utc");
  await page.keyboard.press("Enter");
  await page.keyboard.type("berlin");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "移除 UTC" })).toBeVisible();
  await expect(page.getByRole("button", { name: "移除 Europe/Berlin" })).toBeVisible();
  await page.keyboard.press("Escape");
  expect(external).toHaveLength(0);
});

test("Menu：Enter 打开、↓↓ 移动、Enter 执行", async ({ page, external }) => {
  const trigger = page.getByTestId("menu-trigger");
  await tabTo(page, trigger);
  expect(await hasRing(page)).toBe(true);
  await page.keyboard.press("Enter");
  await expect(trigger).toHaveAttribute("aria-expanded", "true");
  await expect(page.locator(":focus")).toHaveAttribute("role", "menu");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("last-action")).toContainText("菜单 disarm");
  expect(external).toHaveLength(0);
});

test("命令面板：⌘K/按钮打开、输入过滤、↓ + Enter 执行、ESC 关闭", async ({ page, external }) => {
  await tabTo(page, page.getByRole("button", { name: "命令面板" }));
  await page.keyboard.press("Enter");
  const input = page.getByRole("combobox", { name: "输入命令或页面名" });
  await expect(input).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("last-action")).toContainText("命令 资产");
  await page.getByRole("button", { name: "命令面板" }).focus();
  await page.keyboard.press("Enter");
  await page.keyboard.type("主题");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("last-action")).toContainText("命令 切换主题");
  await page.getByRole("button", { name: "命令面板" }).focus();
  await page.keyboard.press("Enter");
  await page.keyboard.press("Escape");
  await expect(input).toBeHidden();
  expect(external).toHaveLength(0);
});

test("Dialog：打开后聚焦第一个输入，ESC 关闭并归还焦点", async ({ page, external }) => {
  const opener = page.getByRole("button", { name: "打开对话框" });
  await tabTo(page, opener);
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("dialog-input")).toBeFocused();
  expect(await hasRing(page)).toBe(true);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toBeHidden();
  await expect(opener).toBeFocused();
  expect(external).toHaveLength(0);
});

test("AlertDialog：role=alertdialog，初始焦点在取消，Tab 到确认后 Enter 执行", async ({ page, external }) => {
  await tabTo(page, page.getByRole("button", { name: "删除" }).first());
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("alertdialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("button", { name: "取消" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(dialog.getByRole("button", { name: "删除" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("last-action")).toContainText("确认删除");
  expect(external).toHaveLength(0);
});

test("Drawer：Enter 打开、Tab 在内部循环、ESC 关闭", async ({ page, external }) => {
  const opener = page.getByRole("button", { name: "打开抽屉" });
  await tabTo(page, opener);
  await page.keyboard.press("Enter");
  const drawer = page.getByRole("dialog", { name: "节点详情" });
  await expect(drawer).toBeVisible();
  for (let step = 0; step < 4; step += 1) {
    await page.keyboard.press("Tab");
    expect(await drawer.evaluate((element) => element.contains(document.activeElement))).toBe(true);
  }
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
  expect(external).toHaveLength(0);
});

test("应用外壳：⌘K 打开命令面板并跳转页面；Theme Editor 可用键盘打开与关闭", async ({ page, external }) => {
  await page.goto("/");
  await expect(page.locator(".breadcrumb")).toContainText("总览");
  await page.keyboard.press("ControlOrMeta+k");
  const input = page.getByRole("combobox", { name: "输入命令或页面名" });
  await expect(input).toBeFocused();
  await page.keyboard.type("审计");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/audit$/u);
  await tabTo(page, page.getByRole("button", { name: "主题" }));
  await page.keyboard.press("Enter");
  const editor = page.getByRole("dialog", { name: "主题" });
  await expect(editor).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(editor).toBeHidden();
  expect(external).toHaveLength(0);
});

test("每个可聚焦元素都有可见的 :focus-visible 环", async ({ page, external }) => {
  const missing: string[] = [];
  for (let step = 0; step < 80; step += 1) {
    await page.keyboard.press("Tab");
    const name = await page.evaluate(() => {
      const element = document.activeElement;
      if (element === null || element === document.body) return "";
      return `${element.tagName.toLowerCase()}.${(element.getAttribute("class") ?? "").slice(0, 40)}[${(element.getAttribute("aria-label") ?? element.textContent).slice(0, 20)}]`;
    });
    if (name !== "" && !(await hasRing(page))) missing.push(name);
  }
  expect(missing).toEqual([]);
  expect(external).toHaveLength(0);
});
