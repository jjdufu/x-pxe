import { expect, mockApi, PAGES, test } from "./fixtures";

test("生产入口全部页面与样例页零外部请求", async ({ page, external }) => {
  await mockApi(page);
  for (const item of PAGES) {
    await page.goto(item.path);
    await expect(page.locator(".breadcrumb")).toContainText(item.name);
  }
  await page.goto("/gallery.html");
  await expect(page.getByRole("heading", { name: "组件样例" })).toBeVisible();
  expect(external).toHaveLength(0);
});

test("CSP 下可运行：无内联脚本，主题在挂载前由同源 theme-boot.js 应用", async ({ page, external }) => {
  await mockApi(page);
  await page.route("**/*", async (route) => {
    if (route.request().resourceType() !== "document") {
      await route.fallback();
      return;
    }
    const response = await route.fetch();
    const headers = { ...response.headers(), "content-security-policy": "default-src 'self'; frame-ancestors 'none'" };
    await route.fulfill({ response, headers });
  });
  const violations: string[] = [];
  page.on("console", (message) => {
    if (/Content Security Policy/u.test(message.text())) violations.push(message.text());
  });
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "xpxe.theme.v1",
      JSON.stringify({ v: 1, mode: "dark", gray: "slate", radius: 12, accent: { kind: "preset", name: "blue" }, applied: { light: { accent: "#2563eb", accentFg: "#ffffff" }, dark: { accent: "#2563eb", accentFg: "#ffffff" } } }),
    );
  });
  await page.goto("/nodes");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.locator("html")).toHaveAttribute("data-gray", "slate");
  await expect(page.getByRole("table", { name: "节点" })).toBeVisible();
  expect(violations).toEqual([]);
  expect(external).toHaveLength(0);
});
