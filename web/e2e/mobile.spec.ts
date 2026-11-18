// 门禁第 10 项：375 px 与 767 px 下每个页面无横向溢出；移动端侧栏为离屏抽屉。
import { expect, mockApi, PAGES, test } from "./fixtures";

for (const width of [375, 767]) {
  test(`视口 ${String(width)} px 无横向溢出`, async ({ page, external }) => {
    await page.setViewportSize({ width, height: 800 });
    await mockApi(page);
    for (const item of [...PAGES.map((entry) => entry.path), "/gallery.html"]) {
      await page.goto(item);
      await page.waitForLoadState("networkidle");
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      expect(overflow, `${item} 横向溢出 ${String(overflow)} px`).toBeLessThanOrEqual(0);
    }
    await page.goto("/nodes");
    const menu = page.getByRole("button", { name: "打开导航" });
    await expect(menu).toBeVisible();
    await menu.click();
    await expect(page.locator(".sidebar")).toHaveClass(/open/u);
    await page.getByRole("link", { name: "资产" }).click();
    await expect(page.locator(".sidebar")).not.toHaveClass(/open/u);
    expect(external).toHaveLength(0);
  });
}
