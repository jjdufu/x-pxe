// Step 14 §3.5：ISO 标签页与手动引导菜单页（路由拦截，离线）。
import { expect, mockApi, test } from "./fixtures";

const PVE_SHA = "e3b8c3f1".repeat(8);

test("ISO 标签页：列表与识别结果；删除前列出引用条目", async ({ page, external }) => {
  const api = await mockApi(page);
  await page.goto("/assets");
  await page.getByRole("tab", { name: "ISO 镜像" }).click();
  const main = page.locator("main#main");
  await expect(main.getByRole("cell", { name: "proxmox-ve_9.2-1" })).toBeVisible();
  await expect(main.getByText("Proxmox VE 系")).toBeVisible();
  await page.getByRole("button", { name: `操作 ${PVE_SHA}` }).click();
  await page.getByRole("menuitem", { name: "删除" }).click();
  const dialog = page.getByRole("alertdialog");
  await expect(dialog.getByTestId("iso-refs")).toHaveText("pve、rescue");
  await dialog.getByRole("button", { name: "删除" }).click();
  await expect.poll(() => api.writes()).toContain(`DELETE /api/v1/isos/${PVE_SHA}`);
  expect(external).toHaveLength(0);
});

test("手动引导菜单：键盘 ↑/↓ 排序更新预览并保存", async ({ page, external }) => {
  const api = await mockApi(page);
  await page.goto("/boot-menu");
  const preview = page.getByLabel("menu.ipxe 预览");
  await expect(preview).toContainText(/m-pve[\s\S]*m-rescue/u);
  const handle = page.getByRole("button", { name: "移动 Proxmox VE 9.2（↑/↓）" });
  await handle.focus();
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("button", { name: "移动 Proxmox VE 9.2（↑/↓）" })).toBeFocused();
  await expect(preview).toContainText(/m-rescue[\s\S]*m-pve/u);
  await page.getByRole("button", { name: "保存", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "保存配置" });
  await dialog.getByLabel("变更说明").fill("调整顺序");
  await dialog.getByRole("button", { name: "保存" }).click();
  await expect.poll(() => api.writes()).toContain("PUT /api/v1/configs/boot_menu/7");
  expect(external).toHaveLength(0);
});

test("手动引导菜单：开启需 AlertDialog 确认；cmdline 白名单校验", async ({ page, external }) => {
  await mockApi(page);
  await page.goto("/boot-menu");
  const toggle = page.getByRole("checkbox", { name: "启用手动引导菜单" });
  await expect(toggle).toBeChecked();
  await page.getByText("启用手动引导菜单").click();
  await expect(toggle).not.toBeChecked();
  await page.getByText("启用手动引导菜单").click();
  const confirm = page.getByRole("alertdialog");
  await expect(confirm).toContainText("未武装且命中白名单");
  await confirm.getByRole("button", { name: "确认" }).click();
  await expect(toggle).toBeChecked();
  await page.getByRole("button", { name: "编辑 Proxmox VE 9.2" }).click();
  const drawer = page.getByRole("dialog", { name: "编辑菜单条目" });
  await drawer.getByLabel("内核命令行").fill("quiet; reboot");
  await expect(drawer.getByText("只允许字母、数字、空格与", { exact: false }).last()).toBeVisible();
  await expect(drawer.getByRole("button", { name: "应用" })).toBeDisabled();
  expect(external).toHaveLength(0);
});
